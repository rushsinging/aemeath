# Server 设计

## 定位

Server 是**入站适配器**——把远端客户端的 WebSocket 请求转换为核心域调用。与 TUI 同级，共享同一个 `AgentClient` 端口契约，区别只在传输层：TUI 走进程内直调，Server 走 WebSocket 代理。

## 六边形端口

```
  Remote Client                  Control Plane                Worker
  (CLI --server)                 (aemeath serve)              (per-session)
       │                              │                          │
       │ WS (TCP)                     │ WS (uds)                 │
       ▼                              ▼                          ▼
  ┌──────────┐               ┌────────────────┐         ┌──────────────┐
  │ Wire-    │               │  WsProxy       │         │  serve_ws    │
  │ Client   │──session_id──▶│  (薄代理)       │──透传──▶│  (worker侧)  │
  │ (SDK)    │               │  不反序列化帧    │         └──────┬───────┘
  └──────────┘               └────────────────┘                │
                                     │                         ▼
                              ┌──────┴───────┐         ┌──────────────┐
                              │ Session-     │         │  Runtime     │
                              │ Manager      │         │  (核心域)     │
                              └──────────────┘         └──────────────┘
```

| 端口 | 本 MVP adapter | 后续可换 |
|---|---|---|
| `AgentClient`（入站） | `AgentClientImpl`（本地）/ `serve_ws`（worker WS server）/ `WireClient`（CLI WS client） | — |
| `WorkerLauncher` | `LocalProcessLauncher` | Container / Remote（跨机） |
| `Storage` | 文件式（现有） | 中心 DB adapter |
| `Transport` | WS-over-uds（控制面↔worker）/ WS-over-TCP（CLI↔控制面） | 跨机：WS-over-TCP |

**关键**：`Call`/`Resp` 是**唯一一套协议**，跑在 WS 上。控制面不解析帧、只转发。

## 进程拓扑

同一个 `aemeath` 二进制，三种角色：

```
CLI（双模式，TUI 不变）
 ├─ 直连:   AgentClientImpl（本地 Runtime，进程内直调）  ← 现有行为
 └─ server: WireClient ──WS(TCP)──┐
                                   ▼
              控制面进程  aemeath serve（常驻，1 个）
              WsProxy: 终结 client WS、auth / 路由
              SessionManager: session_id → WorkerHandle
              WorkerLauncher（LocalProcess）
                    │
                    ├── worker 进程 A（会话A，uds WS）
                    ├── worker 进程 B（会话B，uds WS）
                    └── ...
```

- 控制面 = 1 个进程；worker = N 个进程（每活跃会话一个），靠 WS 通信
- Worker 自己是 WS server（监听 uds），控制面把 client 的 WS 代理到对应 worker 的 WS

## AgentClient-over-WS 协议

> **草案状态**：Server 设计是 **Future / 方向预留**，不属于 v0.1.0 范围。以下协议描述需要与 AgentClient trait 的 Target 形态（含 `set_reasoning_level` / `reply_interaction` / `cancel_interaction`）对齐后才能作为可实施设计。

一条 WS 连接 = 一个 session 通道，`req_id` 多路复用，流式响应多帧：

```rust
// Target AgentClient 方法子集的 WS 封装（非新 RPC）
pub enum Call {
    Chat(ChatRequest),
    Cancel { run_id: RunId },         // 必须携 RunId，映射 cancel_run(RunId)
    ReplyInteraction { request_id: InteractionRequestId, reply: InteractionReply },
    CancelInteraction { request_id: InteractionRequestId, reason: InteractionCancelReason },
    SetReasoningLevel { level: ReasoningLevel },
    // Session 管理由控制面负责，不在帧协议中
}

pub enum Resp {
    Unit, Err(SdkError),
    ChatEvent(ChatEvent),
}

pub struct Frame { pub req_id: u64, pub body: FrameBody }
```

`Call`/`Resp` 内全是现有 serde 的 SDK 类型，几乎不造新 DTO。

两侧复用同一份代码：
- **client 侧** `WireClient`（实现 `AgentClient`）：序列化 `Call` 发出、按 `req_id` 路由 `Resp`、把 `ChatEvent` 帧还原成 `ChatStream`
- **server 侧** `serve_ws(client, ws)`：读 `Call` → 调真 `AgentClient` → 写 `Resp`

```rust
pub trait WsConn: Send {
    async fn send(&mut self, f: Frame) -> io::Result<()>;
    async fn recv(&mut self) -> io::Result<Option<Frame>>;
}
```

## Worker 侧

worker = 现有 runtime + 一个 WS server，**runtime 一行不改**：

```rust
let client = composition::build_agent_client(args).await?;  // AgentClientImpl(Single)
let listener = UnixListener::bind(worker_uds_path())?;
ws_accept_loop(listener, move |ws| {
    let client = client.clone();
    async move { serve_ws(client, ws).await }
}).await;
```

- `serve_ws`：循环读 `Call` → 调真 `client` 的方法 → 写 `Resp`。`Chat` 与 `SubscribeChanges` 逐帧转发；其余 ~12 方法直白 req→resp
- worker 只监听 uds（不占 TCP 端口）

## 控制面侧

控制面**不持有 `dyn AgentClient`、不翻译协议**——只代理 WS：

```rust
pub struct SessionManager {
    launcher: Arc<dyn WorkerLauncher>,
    registry: Mutex<HashMap<SessionId, WorkerHandle>>,  // MVP: 内存
}

#[async_trait]
pub trait WorkerLauncher: Send + Sync {
    async fn launch(&self, s: &SessionId, cfg: &WorkerConfig) -> Result<WorkerHandle>;
}
pub struct LocalProcessLauncher;
// launch: 选 uds 路径 → Command::new(current_exe).arg("worker").env(WS_UDS=path)
```

**WsProxy**（公网入口）：

```rust
async fn on_client_ws(client_ws, session_id) {
    let ws_uds = session_mgr.worker_for(&session_id).await?;  // 找/拉起 worker
    let worker_ws = connect_uds_ws(&ws_uds).await?;           // 连 worker 的 WS
    pipe_bidirectional(client_ws, worker_ws).await;            // 双向透传帧，不解析
}
```

控制面在连接边缘做：连接级 auth 校验（WS handshake）、`session_id` 路由。**帧内容一律透传，不反序列化**。帧级认证由 worker 侧经 AgentClient 校验。

## CLI 双模式

```rust
let client: Arc<dyn AgentClient> = match mode {
    Mode::Local          => composition::build_agent_client(cfg, args).await?,
    Mode::Server { url } => Arc::new(ServerSessionClient::connect(url, session_id).await?),
};
run_tui(client).await?;  // TUI 不变
```

模式来源：`--server <url>` flag 或 `aemeath.json` 的 `server` 段；缺省 = 本地直连。

## 多 Agent 维度（为 swarm 预留）

即使 MVP runtime 是 Single，现在就把 agent 维度设计进契约：

```rust
pub type AgentId = String;  // Single 模式恒为 "main"

pub enum ChatEvent {
    Token { agent: AgentId, text: String },
    ToolCallStart { agent: AgentId, name: String, index: usize },
    // ...
}
pub struct SessionSnapshot {
    pub agents: Vec<AgentView>,  // Single: len==1
}
```

Single 模式所有事件 `agent = "main"`，退化无感；Swarm 后续填充多 agent，契约不变。

## 数据流（server 模式一轮 chat）

```
CLI 输入回车
→ WireClient.chat(req)           [发 Call::Chat]
→ WS(TCP) 到控制面 WsProxy
→ 控制面: 按 session_id 找 worker uds → 透传帧到 worker WS
→ worker serve_ws 收 Call::Chat → AgentClientImpl.chat(req) → 跑 Agent Loop
→ 每个 ChatEvent → Resp::ChatEvent → worker WS → 控制面透传 → CLI WS
→ WireClient stream → TUI 渲染
→ ChatEvent::Done 终止
```

## 存储（MVP）

| 项目 | 方案 | 后续 |
|---|---|---|
| Session 存储 | 现有文件式 Storage BC，per-session 目录 | 中心 DB adapter |
| 控制面注册表 | 内存（重启丢失，MVP 可接受） | 持久化 |
| Workspace | per-session 目录（本机） | 卷/对象存储 |
| 中心 DB | 不在 MVP | 后续 storage-backend 子项目 |

## 失败处理

| 场景 | 处理 |
|---|---|
| Worker 崩溃 | 控制面以 `ChatEvent::Error` 收尾；下次请求重 launch + `load_session` 从文件恢复 |
| Worker 空闲 | SessionManager 空闲 N 分钟回收进程 |
| CLI WS 断开 | 重连带同一 `session_id` re-attach |
| 控制面重启 | 内存注册表丢失；client 重连按 session_id 重新 launch + load_session |
| Cancel | `Call::Cancel` 透传到 worker → `client.cancel()` |

## 架构边界约定

| 范围 | 实体 | 归属 | 生命周期 |
|---|---|---|---|
| Session 级 | 对话 / Turn / Agent Loop / workspace | Worker + Session 存储 | 易逝 |
| 账户/项目级 | Requirement / Project / Task / 团队 | 独立"协作域" BC（新服务，自有 DB） | 长命、跨会话 |
| 基础设施级 | session 注册表 / worker 调度 / 配额 | 控制面 | 进程级 |

**控制面保持薄**——只做路由 / 调度 / 隔离 / 代理，**NEVER** 承载领域实体或业务规则。账户/项目级实体归独立协作域 BC，与控制面是平级 peer。分析在 worker、实体在协作域。

## 新增 crate

| crate | 职责 |
|---|---|
| `packages/agent-wire` | `Call` / `Resp` / `Frame` codec + `WsConn` + `WireClient` + `serve_ws` |
| `apps/server` | `aemeath serve`、`WsProxy`、`SessionManager`、`WorkerLauncher` |

## 非目标（defer）

帧级认证 / 多租户隔离、中心 DB、真沙箱（容器 / microVM）、跨机 / 控制面 HA、资源治理、swarm。

## 参考文档

- [Server Foundation MVP](../../../superpowers/specs/2026-06-01-server-foundation-mvp-design.md)
