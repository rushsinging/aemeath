# Server 总设计

> 来源：[Server Foundation MVP 设计](superpowers/specs/2026-06-01-server-foundation-mvp-design.md)

## 概述

将 Aemeath 从单机 CLI 扩展为**多租户、硬隔离**的 agent server。MVP 只证整条管道——控制面、worker 协议、CLI 双模式——用最小实现让它真能跑、能 dogfood。所有跨部署会变的东西设计成可插拔端口。

## 进程拓扑

```
CLI（双模式，TUI 不变）
 ├─ 直连:   AgentClientImpl（本地 runtime，进程内直调）
 └─ server: ServerSessionClient ──WS(TCP)──┐
                                            ▼
              控制面进程  aemeath serve（常驻）
              WsProxy: 终结 client WS、auth/路由（不解析帧内容）
              SessionManager: session_id → WorkerHandle
              WorkerLauncher（LocalProcess）
                    │
                    ├── worker 进程 A（会话A，uds WS）
                    ├── worker 进程 B（会话B，uds WS）
                    └── ...
```

- 同一个 `aemeath` 二进制，三种角色：默认（CLI）/ `serve`（控制面）/ `worker`。
- 控制面 = 1 个进程；worker = N 个进程（每活跃会话一个），靠 WS 通信。
- worker 自己是个 WS server（监听 uds），控制面把 client 的 WS 代理到对应 worker 的 WS。

## 核心决策

| 决策 | 内容 |
|---|---|
| 硬隔离 | 每会话独立 worker 进程/沙箱 |
| 单一协议 | `AgentClient`-over-WS（`Call`/`Resp`/`Frame`），前门（TCP）与后轴（uds）同一套协议 |
| 控制面薄代理 | 只做路由/调度/隔离/代理，帧内容一律透传不反序列化 |
| worker 自托管 WS | worker = 现有 runtime + WS server，runtime 一行不改 |
| CLI 双模式 | `--server <url>` 连远端 / 缺省本地直连，composition 注入切换 |
| 契约预留多 agent | `ChatEvent`/`SessionSnapshot` 带 `AgentId`，Single 模式退化为 `"main"` |

## AgentClient-over-WS 协议

唯一一套协议，跑在 WS 上：一条 WS 连接 = 一个 session 通道，`req_id` 多路复用，流式响应多帧。

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

- `WireClient`（client 侧，实现 `AgentClient`）+ `serve_ws`（server 侧），worker / CLI / 控制面共享。
- 控制面 `WsProxy`：按 session_id 找 worker uds → 连 worker WS → 双向透传帧，不反序列化。

## 存储（MVP）

- **session 存储**：worker 用现有文件式 Storage，per-session 目录。
- **控制面注册表**：内存（重启丢失，MVP 可接受）。
- **中心 DB**：不在 MVP，后续 storage-backend 子项目。

## 失败处理

| 场景 | 处理 |
|---|---|
| worker 崩溃 | 控制面以 `ChatEvent::Error` 收尾；下次请求重 launch + `load_session` 从文件恢复 |
| worker 空闲 | SessionManager 空闲 N 分钟回收进程 |
| CLI WS 断开 | 重连带同一 `session_id` re-attach |
| 控制面重启 | 内存注册表丢失；client 重连按 session_id 重新 launch |

## 新增 crate

| crate | 职责 |
|---|---|
| `packages/agent-wire` | `Call`/`Resp`/`Frame` codec + `WsConn` + `WireClient` + `serve_ws` |
| `apps/server` | `aemeath serve`、`WsProxy`、`SessionManager`、`WorkerLauncher` |

## 架构边界约定（前瞻）

| scope | 实体 | 归属 |
|---|---|---|
| session 级 | 对话/Turn/Agent Loop/workspace | worker + session 存储 |
| 账户/项目级 | Requirement/Project/Task/团队 | 独立"协作域"BC（新服务，自有 DB） |
| 基础设施级 | session 注册表/worker 调度/配额 | 控制面 |

**控制面保持薄**——只做路由/调度/隔离/代理，**NEVER** 承载领域实体或业务规则。复杂协作逻辑一律不进控制面。

## 非目标（defer）

认证/多租户隔离、中心 DB、真沙箱（容器/microVM）、跨机/控制面 HA、资源治理、swarm 均 defer 到后续子项目。
