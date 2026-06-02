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
2. 控制面进程 `aemeath serve`：管理 worker 生命周期、按 session 反向代理，**不碰对话内容**（只转发帧 + 边缘 auth/路由）。
3. worker 进程 `aemeath worker`：复用现有 runtime（`AgentClientImpl`），**自托管一个 WS server** 暴露 `AgentClient`，**runtime 一行不改**。
4. worker 协议 **B（worker 自托管 WS + 控制面反向代理）**：worker = AgentClient-over-WS 的 **server**；控制面 = WS **反向代理**；**前门（CLI↔控制面）与后轴（控制面↔worker）同一套 WS 协议**，只是传输分别走 TCP 与 uds。
5. `WorkerLauncher` 端口 + `LocalProcessLauncher`（唯一实现）。
6. `AgentClient` 契约**预留多 agent 维度**（`agent_id`），为 swarm 留位，本 MVP 跑 Single 模式。

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
│   ├─ 直连:   AgentClientImpl（本地 runtime，进程内直调）     │
│   └─ server: ServerSessionClient ──WS(TCP)──┐              │
└──────────────────────────────────────────────┼────────────┘
                                                ▼
              ┌─ 控制面进程  aemeath serve（1 个，常驻）────────┐
              │  WsProxy（反向代理）: 终结 client WS、做 auth/路由 │
              │  SessionManager: session_id → WorkerHandle       │
              │  WorkerLauncher（LocalProcess）                   │
              └────────┬──── spawn 子进程 + 转发 WS(uds) ─────────┘
                       ├──▶ worker 进程 A  aemeath worker（会话A）
                       │      WS server ← AgentClientImpl(Single) + 文件 Storage
                       ├──▶ worker 进程 B  aemeath worker（会话B）
                       └──▶ ...（一会话一进程）
```

- **控制面 = 1 个进程**；**worker = N 个进程**（每活跃会话一个），分开的 OS 进程，靠 WS 通信。
- 同一个 `aemeath` 二进制，三种角色：默认（CLI）/ `serve`（控制面）/ `worker`。
- **worker 自己是个 WS server**（监听一个 uds），控制面把 client 的 WS **代理**到对应 worker 的 WS——前后**同一套 AgentClient-over-WS 协议**。

### 2.2 六边形端口

`AgentClient` 是**入站端口**，在每一层复现；跨部署会变的是**出站端口**。

| 端口 | 本 MVP 的 adapter | 后续可换 |
|---|---|---|
| `AgentClient`（入站） | `AgentClientImpl`（本地直调）/ worker 的 AgentClient-over-WS **server** / `ServerSessionClient`（CLI WS **client**） | — |
| `WorkerLauncher` | `LocalProcessLauncher` | Container / Remote（跨机） |
| `Storage` | 文件式（现有） | 中心 DB adapter |
| `Transport`（WS 之下） | WS-over-uds（控制面↔worker）/ WS-over-TCP（CLI↔控制面） | 跨机 worker：WS-over-TCP |

**关键**：`Call`/`Resp` 是**唯一一套协议**，跑在 WS 上。worker 是 server、CLI 是 client、控制面是代理——三者共享同一套 AgentClient-over-WS；**控制面不解析帧、只转发**（auth/路由在连接边缘做）。

---

## 3. 核心决策（已锁定）

1. 多租户 · **硬隔离**（每会话独立 worker 进程/沙箱）。
2. 控制面 = 反向代理/路由（不碰内容）· worker = 完整 runtime（分进程）。
3. worker 协议 **B**（worker 自托管 AgentClient-over-WS server + 控制面反向代理；前门/后轴同一套 WS 协议）。
4. `WorkerLauncher` 可插拔（Local 先 → remote/container 后）。
5. session 存储 → **中心 DB**（worker 直连 + 租户作用域凭证）；**MVP 先用文件式**，DB 是后续 adapter。workspace → 卷/对象存储，不进 DB。
6. multi-main-agent = worker 内 swarm（in-worker），**契约 (a) 单一 agent-aware 契约**，single 为退化态。
7. CLI 双模式（直连 + server），composition 选 adapter。
8. hexagonal port/adapter，顺 047 feature-boundary 延伸到部署层。

---

## 4. 组件设计

### 4.1 AgentClient-over-WS 协议（`packages/agent-wire`，worker/控制面/CLI 共享）

唯一一套协议，跑在 WS 上：一条 WS 连接 = 一个 session 通道，承载 `Call`/`Resp` 消息（serde；req-resp 按 `req_id` 多路复用，流式响应多帧）。

```rust
// 镜像 AgentClient 方法
pub enum Call {
    SessionSnapshot, Cost, TaskList, Project,
    Chat(ChatRequest), Cancel,
    SaveSession, LoadSession(String), ListSessions, DeleteSession(String),
    Compact, SwitchModel(ModelSelector),
    SubscribeChanges,            // 打开 ChangeSet 订阅流
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

> `Call`/`Resp` 内全是**现有 serde 的 sdk 类型**，**几乎不造新 DTO**。**B 里这套就是唯一协议**——没有独立 IPC 层，前门和后轴都跑它。

**两侧复用同一份代码**：
- **client 侧** `WireClient`（实现 `AgentClient`）：把每次方法调用序列化成 `Call` 发出、按 `req_id` 路由 `Resp`、把 `ChatEvent` 帧还原成 `ChatStream`、把 `Change` 帧桥成 `watch::Receiver`。**CLI 的 `ServerSessionClient` = `WireClient` over WS(TCP)**。
- **server 侧** `serve_ws(client, ws)`：读 `Call` → 调真 `AgentClient` → 写 `Resp`。**worker 用它 over WS(uds)**。

```rust
pub trait WsConn: Send {            // 一条 WS 连接的收发抽象
    async fn send(&mut self, f: Frame) -> io::Result<()>;
    async fn recv(&mut self) -> io::Result<Option<Frame>>;
}
// MVP：TokioTungstenite over TCP（前门）/ over uds（控制面↔worker）
```

### 4.2 worker 侧（自托管 WS server）

worker = 现有 runtime + 一个 WS server：

```rust
// aemeath worker 入口
let client = composition::build_agent_client(args).await?;   // AgentClientImpl(Single)，runtime 不改
let listener = UnixListener::bind(worker_uds_path())?;        // 监听 uds，路径由控制面传入
ws_accept_loop(listener, move |ws| {
    let client = client.clone();
    async move { serve_ws(client, ws).await }                // 每连接一个 serve_ws
}).await;
```

- `serve_ws`：循环读 `Call` → 调真 `client` 的方法 → 写 `Resp`。`Chat`（ChatStream）与 `SubscribeChanges`（watch）**逐帧转发**；其余 ~12 方法直白 req→resp。
- worker 只监听 **uds**（不占 TCP 端口）；控制面通过这条 uds 与它通。

### 4.3 控制面侧（反向代理 + 调度，**无 RemoteAgentClient**）

控制面**不持有 `dyn AgentClient`、不翻译协议**——只代理 WS：

```rust
pub struct SessionManager {
    launcher: Arc<dyn WorkerLauncher>,
    registry: Mutex<HashMap<SessionId, WorkerHandle>>,   // MVP: 内存
}
pub struct WorkerHandle { pub ws_uds: PathBuf, child: ChildProcess }   // 注意：是 WS 地址，不是 AgentClient

impl SessionManager {
    // 首次用到 → launch（拿到 worker 的 uds）；崩溃 → 重 launch；空闲 → 回收
    pub async fn worker_for(&self, s: &SessionId) -> Result<PathBuf /*ws_uds*/>;
}

#[async_trait]
pub trait WorkerLauncher: Send + Sync {
    async fn launch(&self, s: &SessionId, cfg: &WorkerConfig) -> Result<WorkerHandle>;
}
pub struct LocalProcessLauncher;
// launch: 选一个 uds 路径 → Command::new(current_exe).arg("worker").env(WS_UDS=path)
//         → 等 worker 就绪 → WorkerHandle{ ws_uds: path, child }
```

**WsProxy**（公网入口）：

```rust
// 控制面 axum WS endpoint
async fn on_client_ws(client_ws, session_id, /* auth ctx */) {
    let ws_uds = session_mgr.worker_for(&session_id).await?;   // 找/拉起 worker
    let worker_ws = connect_uds_ws(&ws_uds).await?;            // 连 worker 的 WS
    pipe_bidirectional(client_ws, worker_ws).await;           // 双向**透传帧**，不解析
}
```

控制面在**连接边缘**做：auth 校验、`session_id` 路由、（后续）限流/计费按帧计数。**帧内容（Call/Resp）一律透传，不反序列化**——真正"不碰内容"。

### 4.4 公网 wire + CLI `ServerSessionClient`

- **公网入口 = 控制面的 WsProxy**（§4.3）。`ServerSessionClient` 连它，握手带 `session_id`（新建/attach）（+ 后续 auth token）。
- **`ServerSessionClient`（实现 `AgentClient`）= `WireClient` over WS(TCP)**——和 worker 用的是**同一套 `Call`/`Resp` 协议**，只是传输是 TCP-WS、且经控制面代理到 worker。client 侧逻辑（req_id 多路复用、stream 还原、watch 桥接）**写一次，CLI 复用**。

### 4.5 CLI 双模式（composition 分支）

```rust
let client: Arc<dyn AgentClient> = match mode {
    Mode::Local           => composition::build_agent_client(cfg, args).await?,   // 现状，进程内直调
    Mode::Server { url }  => Arc::new(ServerSessionClient::connect(url, session_id).await?),
};
run_tui(client).await?;   // TUI 不变
```

模式来源：`--server <url>` flag 或 `aemeath.json` 的 `server` 段；缺省 = 本地直连（**保持现有行为**）。

### 4.6 AgentClient 契约的多 agent 维度（为 swarm 预留）

即使 MVP runtime 是 `Single`，**现在就把 agent 维度设计进契约**，否则后加 swarm 破 wire：

```rust
pub type AgentId = String;   // Single 模式恒为 "main"

pub enum ChatEvent {
    Token { agent: AgentId, text: String },
    ToolCallStart { agent: AgentId, name: String, index: usize },
    // ... 其余流式事件同样带 agent 字段
    Done(ChatResult),
    Error(AemeathError),
}
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
 → ServerSessionClient.chat(req)        [WireClient, 发 Call::Chat]
 → WS(TCP) 到控制面 WsProxy
 → 控制面: 按 session_id 找 worker uds → 把帧**透传**到 worker WS
 → worker serve_ws 收 Call::Chat → 真 AgentClientImpl.chat(req) → 跑 Agent Loop（调模型/工具…）
 → 每个 ChatEvent → Resp::ChatEvent 帧 → worker WS → 控制面**透传** → CLI WS
 → ServerSessionClient stream → CLI TUI 渲染（按 agent 维度）
 → ChatEvent::Done 终止
```

**本地直连模式**：`AgentClientImpl.chat()` 直调，无任何 WS——同一段 TUI 代码。
**对比 A**：控制面不再 `RemoteAgentClient` 翻译，只**透传帧**；少一层、少一套协议。

---

## 6. 存储（MVP）

- **session 存储**：worker 用**现有文件式 Storage BC**，写入根设为 **per-session 目录**（如 `<server-data>/sessions/<session_id>/.agents/`）。worker 崩溃 → 目录保留 → 重启 + `load_session` 恢复。
- **控制面注册表**：**内存**（控制面重启会丢 session 路由，MVP 可接受；持久化留后续）。
- **workspace**：per-session 目录（本机）。
- **中心 DB**：**不在 MVP**——是后续"storage backend"子项目（换 `shared/adapter/storage` 的 adapter，runtime 不动）。目标 schema（session/messages/tasks/memory/cost_history + 控制面 registry/tenants/auth/quotas，workspace 只存引用）记在该子项目 spec。

---

## 7. 失败与生命周期

| 场景 | 处理 |
|---|---|
| worker 崩溃（中途） | worker WS 断开 → 控制面把 client 当前 chat 流以 `ChatEvent::Error` 收尾；SessionManager 标记会话死；下次请求**重 launch worker + `load_session(id)`** 从文件恢复 |
| worker 空闲 | SessionManager 空闲 N 分钟回收进程（会话状态在文件，可再拉起） |
| CLI WS 断开 | `ServerSessionClient` 标记断开；重连带同一 `session_id` re-attach（控制面重新代理到仍在/被恢复的 worker） |
| 控制面重启（MVP） | 内存注册表丢失；client 重连按 session_id 重新 launch + load_session（依赖文件存储） |
| `Cancel` | `Call::Cancel` 透传到 worker → `client.cancel()`（置 AtomicBool） |

> 真正的健康检查、优雅迁移、控制面 HA 留后续。

---

## 8. 测试策略

1. **协议编解码单测**：`Call`/`Resp`/`Frame` round-trip（serde + 帧）。
2. **`WireClient` 单测**：用内存双工 `WsConn`（`tokio::io::duplex` 包一层）模拟两端，验证 req_id 多路复用、stream 还原、watch 桥接、错误传播。
3. **`serve_ws` 单测**：内存 `WsConn` + mock AgentClient，验证每个 `Call` 正确分发、流式逐帧。
4. **`SessionManager` 单测**：`FakeLauncher` 返回一个 in-proc 跑 `serve_ws(真 AgentClientImpl)` 的 worker（不起进程）→ 验证 launch-on-first-use、崩溃重启、回收。
5. **WsProxy 单测**：内存双工两端，验证按 session 路由 + 双向透传 + 一端断开另一端收尾。
6. **集成测试**：起真 `aemeath worker` 子进程（uds WS）+ 控制面代理，一轮 chat round-trip + 崩溃重启 resume。
7. **CLI 双模式集成**：本地直连 vs server 模式同一段对话，断言 TUI 行为一致。

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

- `packages/agent-wire`（新）：`Call`/`Resp`/`Frame`/codec + `WsConn` 抽象 + **`WireClient`（client 侧，实现 AgentClient）** + **`serve_ws`（server 侧）**。worker / CLI-server / 控制面代理共享。
- `apps/server`（新）：`aemeath serve` 入口、`WsProxy`（反向代理）、`SessionManager`、`WorkerLauncher`/`LocalProcessLauncher`。**不含 RemoteAgentClient**（B 没这东西）。
- `apps/cli`（改）：新增 `aemeath worker` 子命令（复用 composition + `serve_ws` over uds）；composition root 增 `ServerSessionClient`（= `WireClient`）分支 + `--server` flag/config。
- `agent/*`、`packages/sdk`：仅 `sdk` 的 `ChatEvent`/`SessionSnapshot` 加 agent 维度（§4.6）；runtime/features **不动**。

> `apps/server` 是新顶层 app（#36 旧 server 已删，本设计是全新的、薄的代理控制面，不复用任何历史代码）。

---

## 11. 后续子项目（依赖本 MVP）

| 子项目 | 内容 |
|---|---|
| storage-backend | 中心 DB adapter（文件→DB），租户作用域凭证，目标 schema |
| auth-tenancy | 认证、授权、多租户隔离、行级安全 |
| sandbox-launcher | 真沙箱 `WorkerLauncher`（容器/microVM） |
| multi-machine | 跨机 remote launcher（worker WS 改 TCP 暴露）+ 控制面 HA |
| governance | 配额/限流/按租户成本归账（控制面边缘按帧计数） |
| swarm-feature | worker 内 multi-main-agent（coordinator + peer），填充契约 agent 维度 |

> 跨机在 B 下很自然：worker 本就是 WS server，把 uds 换成 TCP 暴露、launcher 换成远程，控制面代理逻辑不变。

---

## 12. 自检（待实施前确认）

- **范围聚焦**：本 MVP 只打通管道，所有可换项以最简 adapter 占位，不夹带后续子项目。
- **契约前瞻**：§4.6 的 agent 维度是唯一对现有 sdk 的破坏性改动，必须随 MVP 落地。
- **行为兼容**：本地直连模式（缺省）行为与现状完全一致；server 模式为新增路径。
- **复用最大化**：`WireClient`（client）+ `serve_ws`（server）一套协议，worker / CLI-server 共享；控制面是薄代理，无第二套协议、无 RemoteAgentClient；worker 复用 composition + runtime。
