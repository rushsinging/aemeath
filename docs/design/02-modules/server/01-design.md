# Server 设计

> **⚠️ 文档状态：Future / 研究输入，不可实施，不属于 v0.1.0 范围。**
>
> 本文档是方向性研究输入，用于探索 Server 模式的可能形态、收集反馈，**不是可以直接编码的设计**。文中出现的 WS / UDS 拓扑、`Call` / `Resp` / `Frame` 协议、`WsProxy` / `SessionManager` 等，**全部是非冻结示意**（illustrative sketch），随时可能整体推翻重画，不构成任何实现契约。
>
> 在「[安全闭环（MUST，先于可实施设计）](#安全闭环must先于可实施设计)」一节列出的每一项被逐一设计、实现、并通过独立安全评审之前，本文档**不得**被视为可实施设计，**任何人不得据此编码**。当前草案中出现的"连接级 auth 校验 + 帧透传 + 帧级认证 defer"的描述**不构成安全方案**，仅是占位/待办标记。**NEVER** 在安全闭环补齐前把 Server 部署到公网或多租户环境。

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

**关键**：`Call`/`Resp` 是**唯一一套协议**，跑在 WS 上。控制面不解析帧、只转发。（协议与拓扑均为非冻结示意，见文首状态说明）

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

> **⚠️ 研究级 sketch，不可作为 Target 契约。** 以下 `Call`/`Resp`/`Frame`/`WsConn`/`pipe_bidirectional` 等 Rust 代码片段 **全部为非冻结、非闭合的研究草图**——enum 未涵盖全部 variant、trait 方法未定义完整错误类型与边界语义、`Frame` 未定义序列化格式/版本号/长度前缀/分帧策略、`pipe_bidirectional` 仅是一个名字未给出任何 backpressure/半关闭/错误传播/重连语义。这些草图 **NEVER** 直接用作实现基线；正式设计 **MAY** 完全推翻重画，不承担任何兼容义务。
>
> 在以下条件**全部满足**之前，本节代码不可升级为 Target 契约：
> 1. `AgentClient` trait 已冻结（Target freeze），所有方法签名与语义不再变更；
> 2. 「[安全闭环（MUST）](#安全闭环must先于可实施设计)」全部项已完成设计、实现并通过独立安全评审；
> 3. wire codec 版本协商、向前兼容、分帧与错误传播策略已完成 RFC 级设计文档。

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

/// FrameBody 是 Call 或 Resp 的枚举包装。
pub enum FrameBody {
    Call(Call),
    Resp(Resp),
}
```

`Call`/`Resp` 内全是现有 serde 的 SDK 类型，无需新 DTO。

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

worker = 现有 runtime + 一个 WS server，**runtime 一行不改**。**关键架构边界**：worker **自行** composition + load——控制面不持有 `dyn AgentClient`、不注入 Storage、不参与 domain 对象构建。控制面仅负责路由/调度/代理，worker 进程内部自行完成 `composition::build_agent_client()` + `load_session()`。

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

控制面在连接边缘**设想**做：连接级 auth 校验（WS handshake）、`session_id` 路由。**帧内容一律透传，不反序列化**。**`session_id` 不是认证凭据**——仅用于路由/会话定位，鉴权/授权必须基于独立 token/credential（见「[安全闭环（MUST）](#安全闭环must先于可实施设计)」），不得以持有或知晓 `session_id` 即视为已认证。

> 上述"auth 校验 + 帧透传"只是占位描述，**不构成安全方案**：未定义 TLS/WSS、未定义 token 签发/过期/吊销/scope、未做 session binding、未做 nonce/timestamp/replay 防护、未做 UDS peer credential 校验、未做消息大小/rate limit/背压/配额/资源隔离/审计。此外，`pipe_bidirectional`/`on_client_ws`/`WsProxy` 等 Rust 代码片段 **均为非闭合研究 sketch**（无 backpressure/半关闭/错误传播/重连语义定义），**不得作为 Target 契约**。帧级认证 / 多租户隔离目前为 **defer**（见非目标），在「[安全闭环（MUST）](#安全闭环must先于可实施设计)」全部补齐并通过安全评审前，**NEVER** 把本设计部署到公网或多租户环境。

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

## 安全闭环（MUST，先于可实施设计）

> 本节列出的每一项，必须逐一给出具体设计、完成实现、并通过独立安全评审后，本文档才能从"Future / 研究输入"升级为可实施设计。**任何一项缺失，NEVER 把 Server 部署到公网或多租户环境**——即便仅是内网/单租户试用，也必须先完成传输安全与 UDS 安全两类。

| 类别 | MUST 项 |
|---|---|
| 传输安全 | TLS/WSS（CLI↔控制面 全链路）+ 证书验证（链校验、主机名校验，视场景加双向 mTLS）；不得存在明文 WS 生产路径 |
| 身份与凭证 | token issuance（签发流程与凭据存储）/ expiry（过期与刷新）/ revocation（吊销与黑名单）/ scope（最小权限作用域限定） |
| 会话完整性 | session binding：token 与 `session_id` / 连接强绑定，防止跨会话越权复用；nonce / timestamp + replay protection：防重放攻击 |
| UDS 安全 | 控制面↔worker UDS 的 peer credential 校验（`SO_PEERCRED`/`LOCAL_PEERCRED`）；socket 文件权限收紧；worker capability / 最小权限约束（禁止越权文件系统 / 网络访问） |
| 资源边界 | 单帧/单连接消息与连接大小上限；per-connection / per-session rate limit；backpressure（慢消费者 / 慢 worker 处理策略）；有界队列（禁止无界内存增长）；per-session / per-tenant 配额；worker 间公平调度；资源隔离（CPU / 内存 / 磁盘 / 网络） |
| 可观测 | 审计日志：连接建立/断开、鉴权成功/失败、异常关闭、配额超限、Cancel 等关键事件可追溯、防篡改 |

当前草案中「控制面在连接边缘做连接级 auth 校验」「帧内容一律透传」「帧级认证 / 多租户隔离为 defer」等描述，**均为占位，不构成安全方案**——未指定 token 生命周期、未做 session binding、未做 replay 防护、未做 UDS peer credential 校验、未定义任何资源边界或审计机制。在上表全部项落地并评审通过之前，不得以"已有 auth 字样"为由认为该设计已具备生产安全性。

## 非目标（defer）

帧级认证 / 多租户隔离、中心 DB、真沙箱（容器 / microVM）、跨机 / 控制面 HA、资源治理、swarm。

> 注：帧级认证 / 多租户隔离虽标记为 defer，但其覆盖范围与「安全闭环（MUST）」中的 token/session binding/资源隔离等项强相关——defer 的是"现在不做"，MUST 的是"可实施设计前必须先做"，两者不矛盾：本设计在 MUST 项补齐前本就不可实施，defer 项自然也未到需要展开的阶段。

## 参考文档

- [Server Foundation MVP](../../../superpowers/specs/2026-06-01-server-foundation-mvp-design.md)
