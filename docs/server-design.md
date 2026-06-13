# Server 设计

> 详细设计稿：[Server Foundation MVP](superpowers/specs/2026-06-01-server-foundation-mvp-design.md)

## 定位

Server 是**入站适配器**——把远端客户端的 WebSocket 请求转换为核心域调用。与 TUI 同级，共享同一个 `AgentClient` 端口契约，区别只在传输层：TUI 走进程内直调，Server 走 WebSocket 代理。

## 端口契约

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

**六边形合规**：
- 控制面只做路由 / 调度 / 隔离 / 代理，**NEVER** 承载领域实体或业务规则。
- Worker 内部是完整的 Runtime 应用服务 + WS server 适配器，Runtime 一行不改。
- 前门（CLI ↔ 控制面 TCP）与后轴（控制面 ↔ worker uds）共享同一套 `AgentClient`-over-WS 协议。

## 进程拓扑

同一个 `aemeath` 二进制，三种角色：

```
CLI（双模式，TUI 不变）
 ├─ 直连:   AgentClientImpl（本地 Runtime，进程内直调）  ← 现有行为
 └─ server: WireClient ──WS(TCP)──┐
                                   ▼
              控制面进程  aemeath serve（常驻）
              WsProxy: 终结 client WS、auth / 路由
              SessionManager: session_id → WorkerHandle
              WorkerLauncher（LocalProcess）
                    │
                    ├── worker 进程 A（会话A，uds WS）
                    ├── worker 进程 B（会话B，uds WS）
                    └── ...
```

- 控制面 = 1 个进程；worker = N 个进程（每活跃会话一个），靠 WS 通信。
- Worker 自己是 WS server（监听 uds），控制面把 client 的 WS 代理到对应 worker 的 WS。

## 核心决策

| 决策 | 内容 | 六边形意义 |
|---|---|---|
| 硬隔离 | 每会话独立 worker 进程 / 沙箱 | 核心域实例物理隔离 |
| 单一协议 | `AgentClient`-over-WS（`Call` / `Resp` / `Frame`） | 端口契约统一，传输层可替换 |
| 控制面薄代理 | 帧内容一律透传不反序列化 | 控制面不是领域层，只做基础设施 |
| Worker 自托管 WS | Runtime + WS server，Runtime 不改 | 入站适配器透明，核心域无感知 |
| CLI 双模式 | `--server <url>` / 缺省本地直连 | 同一端口，不同适配器实现 |
| 契约预留多 Agent | `ChatEvent` / `SessionSnapshot` 带 `AgentId` | 端口契约面向未来扩展 |

## AgentClient-over-WS 协议

一条 WS 连接 = 一个 session 通道，`req_id` 多路复用，流式响应多帧：

```rust
enum Call {
    SessionSnapshot, Cost, TaskList, Project,
    Chat(ChatRequest), Cancel,
    SaveSession, LoadSession(String), ListSessions, DeleteSession(String),
    Compact, SwitchModel(ModelSelector), SubscribeChanges,
}

enum Resp {
    Snapshot(SessionSnapshot), Cost(CostInfo), Tasks(Vec<TaskSummary>),
    Project(ProjectContext), Sessions(Vec<SessionSummary>),
    Unit, Err(SdkError),
    ChatEvent(ChatEvent),   // 流式多帧
    Change(ChangeSet),      // changes 订阅流
}
```

- `WireClient`（client 侧，实现 `AgentClient`）+ `serve_ws`（server 侧），Worker / CLI / 控制面共享。
- 控制面 `WsProxy`：按 session_id 找 worker uds → 连 worker WS → 双向透传帧。

## 架构边界

| 范围 | 实体 | 归属 |
|---|---|---|
| Session 级 | 对话 / Turn / Agent Loop / workspace | Worker + Session 存储 |
| 账户 / 项目级 | Requirement / Project / Task / 团队 | 独立"协作域" BC（新服务，自有 DB） |
| 基础设施级 | session 注册表 / worker 调度 / 配额 | 控制面 |

控制面保持薄——只做路由 / 调度 / 隔离 / 代理，**NEVER** 承载领域实体或业务规则。复杂协作逻辑一律不进控制面。

## 失败处理

| 场景 | 处理 |
|---|---|
| Worker 崩溃 | 控制面以 `ChatEvent::Error` 收尾；下次请求重 launch + `load_session` 从文件恢复 |
| Worker 空闲 | SessionManager 空闲 N 分钟回收进程 |
| CLI WS 断开 | 重连带同一 `session_id` re-attach |
| 控制面重启 | 内存注册表丢失；client 重连按 session_id 重新 launch |

## 新增 crate

| crate | 职责 |
|---|---|
| `packages/agent-wire` | `Call` / `Resp` / `Frame` codec + `WsConn` + `WireClient` + `serve_ws` |
| `apps/server` | `aemeath serve`、`WsProxy`、`SessionManager`、`WorkerLauncher` |

## 非目标（defer）

认证 / 多租户隔离、中心 DB、真沙箱（容器 / microVM）、跨机 / 控制面 HA、资源治理、swarm。
