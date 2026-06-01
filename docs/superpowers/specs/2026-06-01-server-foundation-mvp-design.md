# Server Foundation MVP — 设计文档

**日期**：2026-06-01
**状态**：设计完成，待 review
**范围**：多租户 Server 的**第一份子项目（地基 MVP）**——端到端打通 `CLI --server → 控制面 → worker → 回流`，单机、Single 模式跑通，契约预留多 agent 维度。auth / 中心 DB / 真沙箱 / 跨机 / 资源治理 / swarm 均 defer 到后续子项目。

---

## 1. 概述

把 Aemeath 从单机 CLI 扩展为**多租户、硬隔离**的 agent server：一个**控制面进程**调度**每会话一个 worker 进程**（沙箱内的完整 runtime），CLI 既能本地直连、也能连远端 server。

本 MVP 只证**整条管道**——控制面、worker 协议、CLI 双模式——用最小实现（本机进程 + 文件存储 + 无认证）让它**真能跑、能 dogfood**。所有跨部署会变的东西（沙箱、存储后端、跨机调度、认证）都设计成**可插拔端口**，本 MVP 只实现最简 adapter，后续子项目替换。

### 1.1 目标

1. CLI **双模式**：`AgentClientImpl`（本地直连，现状）/ `ServerSessionClient`（连 server，WS），靠 composition 注入切换，**TUI 不变**。
2. 控制面进程 `aemeath serve`：管理 worker 生命周期、按 session 路由，**不碰对话内容**。
3. worker 进程 `aemeath worker`：复用现有 runtime（`AgentClientImpl`），经 IPC 暴露 `AgentClient`，**runtime 一行不改**。
4. worker 协议 **A（AgentClient-over-IPC）**：`Call`/`Resp` 帧 over 可换 transport（本机 uds）。
5. `WorkerLauncher` 端口 + `LocalProcessLauncher`（唯一实现）。
6. `AgentClient` 契约**预留多 agent 维度**（`agent_id`），为 swarm 留位，本 MVP 跑 Single 模式。
7. 公网 wire（WS）最小实现：session 创建/attach + chat 流。

### 1.2 非目标（defer 到后续子项目）

1. **认证 / 多租户隔离 / 租户作用域 DB 凭证** —— 后续 spec（auth）。MVP 无认证、单租户语义。
2. **中心 DB storage adapter** —— 后续 spec。MVP 用现有文件式 Storage（per-session 目录）。
3. **真沙箱 launcher**（容器/microVM）—— 后续 spec。MVP 只 `LocalProcessLauncher`（无真隔离）。
4. **跨机 remote launcher / 控制面 HA** —— 后续 spec。MVP 单机、单控制面。
5. **资源治理（配额/限流）** —— 后续 spec。
6. **multi-main-agent swarm** —— 独立 runtime feature spec。MVP 跑 `RuntimeMode::Single`，但**契约已预留 agent 维度**。
7. 不改 runtime / Storage BC / 各 feature 的领域逻辑；不恢复任何历史分布式代码。

---

## 2. 架构总览

### 2.1 进程拓扑

```
┌─ CLI（双模式，TUI 不变）──────────────────────────────────┐
│  dyn AgentClient                                           │
│   ├─ 直连:   AgentClientImpl（本地 runtime，进程内）         │
│   └─ server: ServerSessionClient ──WebSocket──┐            │
└────────────────────────────────────────────────┼──────────┘
                                                  ▼
              ┌─ 控制面进程  aemeath serve（1 个，常驻）────────┐
              │  WsGateway（公网 wire）                         │
              │  SessionManager: session_id → WorkerHandle      │
              │  WorkerLauncher（LocalProcess）                  │
              └────────┬──── spawn 子进程 + IPC(uds) ────────────┘
                       ├──▶ worker 进程 A  aemeath worker（会话A）
                       │      AgentClientImpl(Single) + 文件 Storage
                       ├──▶ worker 进程 B  aemeath worker（会话B）
                       └──▶ ...（一会话一进程）
```

- **控制面 = 1 个进程**；**worker = N 个进程**（每活跃会话一个），分开的 OS 进程，靠 IPC 通信。
- 同一个 `aemeath` 二进制，三种角色：默认（CLI）/ `serve`（控制面）/ `worker`。

### 2.2 六边形端口

`AgentClient` 是**入站端口**，在每一层复现；跨部署会变的是**出站端口**。

| 端口 | 本 MVP 的 adapter | 后续可换 |
|---|---|---|
| `AgentClient`（入站） | `AgentClientImpl`（本地）/ `RemoteAgentClient`（控制面→worker）/ `ServerSessionClient`（CLI→server） | — |
| `WorkerLauncher` | `LocalProcessLauncher` | Container / Remote（跨机） |
| `Storage` | 文件式（现有） | 中心 DB adapter |
| `Transport` | uds（本机） | TCP（跨机）/ WS（前门，本 MVP 已用） |

`Call`/`Resp` 一套 codec 跑两种传输：**uds（控制面↔worker）** 与 **WS（CLI↔控制面，外加 auth/路由信封）**。

---

## 3. 核心决策（已锁定）

1. 多租户 · **硬隔离**（每会话独立 worker 进程/沙箱）。
2. 控制面 = 管理/路由（不碰内容）· worker = 完整 runtime（分进程）。
3. worker 协议 **A**（AgentClient-over-IPC）。
4. `WorkerLauncher` 可插拔（Local 先 → remote/container 后）。
5. session 存储 → **中心 DB**（worker 直连 + 租户作用域凭证）；**MVP 先用文件式**，DB 是后续 adapter。workspace → 卷/对象存储，不进 DB。
6. multi-main-agent = worker 内 swarm（in-worker），**契约 (a) 单一 agent-aware 契约**，single 为退化态。
7. CLI 双模式（直连 + server），composition 选 adapter。
8. hexagonal port/adapter，顺 047 feature-boundary 延伸到部署层。

---

## 4. 组件设计

### 4.1 worker 协议（`packages/agent-wire`，控制面与 worker 共享）

帧：`u32` 长度前缀 + serde body（JSON 调试期，可切 bincode）。请求-响应按 `req_id` 多路复用，流式响应多帧。

```rust
// 镜像 AgentClient 方法
pub enum Call {
    SessionSnapshot, Cost, TaskList, Project,
    Chat(ChatRequest), Cancel,
    SaveSession, LoadSession(String), ListSessions, DeleteSession(String),
    Compact, SwitchModel(ModelSelector),
    SubscribeChanges,            // 打开 ChangeSet 订阅流
    Shutdown,                    // 优雅关停 worker
}

pub enum Resp {
    Snapshot(SessionSnapshot), Cost(CostInfo), Tasks(Vec<TaskSummary>),
    Project(ProjectContext), Sessions(Vec<SessionSummary>),
    Unit, Err(SdkError),
    ChatEvent(ChatEvent),        // 流式：多帧，Done/Error 终止
    Change(ChangeSet),           // changes 订阅流
}

pub struct Frame { pub req_id: u64, pub body: FrameBody }  // FrameBody = Call | Resp
```

> `Call`/`Resp` 内全是**现有 serde 的 sdk 类型**（`SessionSnapshot`/`CostInfo`/`ChatEvent`/`ChangeSet`/`ChatRequest`…），**几乎不造新 DTO**。

**Transport 抽象**（让协议与传输解耦）：

```rust
pub trait WireTransport: Send {
    async fn send(&mut self, frame: Frame) -> io::Result<()>;
    async fn recv(&mut self) -> io::Result<Option<Frame>>;
}
// MVP 实现：UdsTransport（控制面↔worker）、WsTransport（CLI↔控制面）
```

### 4.2 worker 侧

**`serve(client, transport)`** —— worker 请求循环：

```rust
pub async fn serve(client: Arc<dyn AgentClient>, mut t: impl WireTransport) {
    while let Some(frame) = t.recv().await? {
        match frame.body.into_call() {
            Call::SessionSnapshot => t.send(resp(frame.req_id, Resp::Snapshot(client.session_snapshot()))).await?,
            Call::Chat(req) => {
                let mut stream = client.chat(req).await?;        // 真 AgentClientImpl
                while let Some(ev) = stream.recv().await {        // ChatStream → 帧流
                    t.send(resp(frame.req_id, Resp::ChatEvent(ev))).await?;
                }
            }
            Call::SubscribeChanges => spawn_change_forwarder(client.changes(), frame.req_id, t.clone()),
            // ... 其余方法 1:1 转发
            Call::Shutdown => break,
        }
    }
}
```

要点：只有 `Chat`（ChatStream）和 `SubscribeChanges`（watch）是**流式**，需逐帧转发；其余 ~12 个方法是直白 req→resp。

**`aemeath worker` 入口**：

```rust
// 复用现有 composition，不改 runtime
let client = composition::build_agent_client(args).await?;   // AgentClientImpl(Single)
let transport = UdsTransport::from_inherited_fd();           // 控制面通过 fd 传入
serve(client, transport).await;
```

### 4.3 控制面侧

**`RemoteAgentClient`（实现 `AgentClient`）** —— 唯一有技术含量的一块：

```rust
pub struct RemoteAgentClient {
    tx: WireSender,                              // 写帧
    pending: Mutex<HashMap<u64, oneshot::Sender<Resp>>>,  // req_id → 等待者
    snapshot_cache: ArcSwap<SessionSnapshot>,   // 本地缓存，由 Change 刷新（无锁读）
    changes_tx: watch::Sender<ChangeSet>,
}

#[async_trait]
impl AgentClient for RemoteAgentClient {
    fn session_snapshot(&self) -> SessionSnapshot { self.snapshot_cache.load_full().as_ref().clone() }
    fn changes(&self) -> watch::Receiver<ChangeSet> { self.changes_tx.subscribe() }
    async fn chat(&self, req: ChatRequest) -> Result<ChatStream> {
        let req_id = self.send(Call::Chat(req)).await?;
        let (tx, rx) = mpsc::unbounded();
        self.route_stream(req_id, tx);          // 后台 reader 按 req_id 把 Resp::ChatEvent 推进 tx
        Ok(ChatStream::from(rx))                 // 还原成同型 ChatStream
    }
    // 同步 getter：读 snapshot_cache（已由常驻 reader 经 Change 帧刷新）
    // 写/查方法：send(Call::..).await → 等 Resp
}
```

机关：
- **一个常驻 reader task** 读 worker 来的所有帧：`Resp::Change` → 刷 `snapshot_cache` + `changes_tx.send`；`Resp(req_id=X)` → 唤醒 `pending[X]` 或推进对应 stream。
- `chat()` 的 `ChatStream` 在控制面侧**还原**；`changes()` 的 watch 在控制面侧**桥接**。

**`WorkerLauncher` 端口 + `LocalProcessLauncher`**：

```rust
#[async_trait]
pub trait WorkerLauncher: Send + Sync {
    async fn launch(&self, session: &SessionId, cfg: &WorkerConfig) -> Result<WorkerHandle>;
}
pub struct WorkerHandle { pub client: Arc<dyn AgentClient>, child: ChildProcess }

pub struct LocalProcessLauncher;
// launch: Command::new(current_exe).arg("worker") + socketpair(uds) 传 fd
//         → 装出 RemoteAgentClient → WorkerHandle{client, child}
```

**`SessionManager`**：

```rust
pub struct SessionManager {
    launcher: Arc<dyn WorkerLauncher>,
    registry: Mutex<HashMap<SessionId, WorkerHandle>>,   // MVP: 内存
}
impl SessionManager {
    // 首次用到 → launch；崩溃 → 重 launch + load_session 恢复；空闲 → 回收
    pub async fn client_for(&self, s: &SessionId) -> Result<Arc<dyn AgentClient>>;
    pub async fn close(&self, s: &SessionId);
}
```

### 4.4 公网 wire（WS）+ `ServerSessionClient`

**控制面 `WsGateway`**：axum WS endpoint。每个 client 连接 = 一个 session 通道：
- 连接时携带 `session_id`（新建或 attach）→ `SessionManager::client_for(session_id)` 取 `dyn AgentClient`。
- 把 WS 消息 ↔ `Call`/`Resp`（复用 4.1 codec，外加最小信封 `{session_id, frame}`）。
- ChatEvent / Change 经 WS 推回 client。

**CLI `ServerSessionClient`（实现 `AgentClient`）**：结构与 `RemoteAgentClient` 几乎相同，只是 transport = `WsTransport` 而非 uds，并在握手时带 `session_id`。**可与 `RemoteAgentClient` 共享大部分代码**（同一套 req_id 多路复用 + stream/watch 桥接），抽成 `WireClient<T: WireTransport>`。

### 4.5 CLI 双模式（composition 分支）

```rust
// apps/cli composition root
let client: Arc<dyn AgentClient> = match mode {
    Mode::Local           => composition::build_agent_client(cfg, args).await?,  // 现状
    Mode::Server { url }  => Arc::new(ServerSessionClient::connect(url, session_id).await?),
};
run_tui(client).await?;   // TUI 不变
```

模式来源：`--server <url>` flag 或 `aemeath.json` 的 `server` 段；缺省 = 本地直连（**保持现有行为**）。

### 4.6 AgentClient 契约的多 agent 维度（为 swarm 预留）

即使 MVP runtime 是 `Single`，**现在就把 agent 维度设计进契约**，否则后加 swarm 破 wire：

```rust
pub type AgentId = String;   // Single 模式恒为 "main"

// 流式事件带 agent 维度
pub enum ChatEvent {
    Token { agent: AgentId, text: String },
    ToolCallStart { agent: AgentId, name: String, index: usize },
    // ... 其余事件同样带 agent 字段
    Done(ChatResult),
    Error(AemeathError),
}

// snapshot 表达多 agent（Single = 单元素）
pub struct SessionSnapshot {
    pub agents: Vec<AgentView>,   // Single: len==1
    // ... 会话级字段
}
```

> Single 模式所有事件 `agent = "main"`，退化无感；Swarm feature 后续填充多 agent，**契约不变**。这是 MVP 必须前瞻落地的一处。

---

## 5. 数据流（端到端：server 模式一轮 chat）

```
CLI 输入回车
 → ServerSessionClient.chat(req)
 → WsTransport 发 {session_id, Call::Chat(req)}
 → 控制面 WsGateway 收 → SessionManager.client_for(session_id)
 → RemoteAgentClient.chat(req) → UdsTransport 发 Call::Chat
 → worker serve 收 → 真 AgentClientImpl.chat(req)  → 跑 Agent Loop（调模型/工具…）
 → 每个 ChatEvent → Resp::ChatEvent 帧 → uds → 控制面 RemoteAgentClient stream
 → WsGateway → WS 帧 → ServerSessionClient stream → CLI TUI 渲染（按 agent 维度）
 → ChatEvent::Done 终止
```

**本地直连模式**：`AgentClientImpl.chat()` 直调，无任何 wire——同一段 TUI 代码。

---

## 6. 存储（MVP）

- **session 存储**：worker 用**现有文件式 Storage BC**，写入根设为 **per-session 目录**（如 `<server-data>/sessions/<session_id>/.agents/`）。worker 崩溃 → 目录保留 → 重启 + `load_session` 恢复。
- **控制面注册表**：**内存**（控制面重启会丢 session 路由，MVP 可接受；持久化留后续）。
- **workspace**：per-session 目录（本机）。
- **中心 DB**：**不在 MVP**——是后续"storage backend"子项目（换 `shared/adapter/storage` 的 adapter，runtime 不动）。

---

## 7. 失败与生命周期

| 场景 | 处理 |
|---|---|
| worker 崩溃（中途） | 当前 chat 流吐 `ChatEvent::Error`；SessionManager 标记会话死；下次请求**重 launch worker + `load_session(id)`** 从文件恢复 |
| worker 空闲 | SessionManager 空闲 N 分钟回收进程（会话状态在文件，可再拉起） |
| CLI WS 断开 | `ServerSessionClient` 标记断开；重连时带同一 `session_id` re-attach（worker 仍在/或被恢复） |
| 控制面重启（MVP） | 内存注册表丢失；client 重连按 session_id 重新 launch + load_session（依赖文件存储） |
| `Cancel` | `Call::Cancel` → worker `client.cancel()`（置 AtomicBool） |

> 真正的健康检查、优雅迁移、控制面 HA 留后续。

---

## 8. 测试策略

1. **协议编解码单测**：`Call`/`Resp`/`Frame` round-trip（serde + 长度前缀）。
2. **`WireClient` 单测**：用内存双工 transport（`tokio::io::duplex`）模拟两端，验证 req_id 多路复用、stream 还原、watch 桥接、错误传播。
3. **`SessionManager` 单测**：`FakeLauncher` 返回 `client = 真 AgentClientImpl`（**in-proc，不起进程**）→ 验证 spawn-on-first-use、崩溃重启、回收。
4. **worker `serve` 单测**：内存 transport + mock AgentClient，验证每个 `Call` 正确分发、流式逐帧。
5. **集成测试**：起真 `aemeath worker` 子进程 + uds，一轮 chat round-trip + 崩溃重启 resume。
6. **CLI 双模式集成**：本地直连 vs server 模式同一段对话，断言 TUI 行为一致。

---

## 9. 明确不做（MVP 边界）

1. 不做认证/授权/租户隔离（无认证、单租户语义）—— 后续 auth spec。
2. 不做中心 DB（用文件存储）—— 后续 storage-backend spec。
3. 不做真沙箱（`LocalProcessLauncher` 无隔离）—— 后续 sandbox spec。
4. 不做跨机/控制面 HA —— 后续 spec。
5. 不做资源治理（配额/限流）—— 后续 spec。
6. 不实现 swarm（跑 `Single`），但**契约预留 agent 维度** —— 后续 swarm feature spec。
7. 不改 runtime / Storage BC / 各 feature 领域逻辑。

---

## 10. crate / 二进制落点

- `packages/agent-wire`（新）：`Call`/`Resp`/`Frame`/`WireTransport`/`WireClient<T>`/codec。控制面与 worker、CLI server 模式共享。
- `apps/server`（新）：`aemeath serve` 入口、`WsGateway`、`SessionManager`、`WorkerLauncher`/`LocalProcessLauncher`、`RemoteAgentClient`。
- `apps/cli`（改）：新增 `aemeath worker` 子命令（复用 composition）；composition root 增 `ServerSessionClient` 分支 + `--server` flag/config。
- `agent/*`、`packages/sdk`：仅 `sdk` 的 `ChatEvent`/`SessionSnapshot` 加 agent 维度（§4.6）；runtime/features **不动**。

> `apps/server` 是新顶层 app（#36 旧 server 已删，本设计是全新的、薄的控制面，不复用任何历史代码）。

---

## 11. 后续子项目（依赖本 MVP）

| 子项目 | 内容 |
|---|---|
| storage-backend | 中心 DB adapter（文件→DB），租户作用域凭证 |
| auth-tenancy | 认证、授权、多租户隔离、行级安全 |
| sandbox-launcher | 真沙箱 `WorkerLauncher`（容器/microVM） |
| multi-machine | 跨机 remote launcher + 传输换 TCP + 控制面 HA |
| governance | 配额/限流/按租户成本归账 |
| swarm-feature | worker 内 multi-main-agent（coordinator + peer），填充契约 agent 维度 |

---

## 12. 自检（待实施前确认）

- **范围聚焦**：本 MVP 只打通管道，所有可换项以最简 adapter 占位，不夹带后续子项目。
- **契约前瞻**：§4.6 的 agent 维度是唯一对现有 sdk 的破坏性改动，必须随 MVP 落地。
- **行为兼容**：本地直连模式（缺省）行为与现状完全一致；server 模式为新增路径。
- **复用最大化**：`RemoteAgentClient` 与 `ServerSessionClient` 共享 `WireClient<T>`；worker 复用 composition + runtime。
