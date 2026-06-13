# Server Foundation MVP 终态

> 完整设计：[`docs/superpowers/specs/2026-06-01-server-foundation-mvp-design.md`](superpowers/specs/2026-06-01-server-foundation-mvp-design.md)

## 核心架构

将 Aemeath 从单机 CLI 扩展为**多租户、硬隔离**的 agent server，端到端打通 `CLI → 控制面 → worker → 回流`。

```
CLI（双模式，TUI 不变）
 ├─ 直连:   AgentClientImpl（本地 runtime，进程内直调）
 └─ server: ServerSessionClient ──WS(TCP)──┐
                                            ▼
              控制面进程  aemeath serve（常驻，反向代理）
              WsProxy: 终结 client WS、做 auth/路由（不解析帧内容）
              SessionManager: session_id → WorkerHandle
              WorkerLauncher（LocalProcess）
                    │
                    ├── worker 进程 A（会话A，uds WS）
                    ├── worker 进程 B（会话B，uds WS）
                    └── ...
```

## 关键决策

| 决策 | 内容 |
|---|---|
| 硬隔离 | 每会话独立 worker 进程/沙箱，控制面不碰对话内容 |
| 单一协议 | `AgentClient`-over-WS（`Call`/`Resp`/`Frame`），前门（CLI↔控制面 TCP）与后轴（控制面↔worker uds）同一套协议 |
| 控制面薄代理 | 只做路由/调度/隔离/代理，帧内容一律透传不反序列化，**NEVER 承载领域实体** |
| worker 自托管 WS | worker = 现有 runtime + WS server，监听 uds，runtime 一行不改 |
| CLI 双模式 | `--server <url>` 连远端 / 缺省本地直连，composition 注入切换，TUI 不变 |
| 契约预留多 agent | `ChatEvent`/`SessionSnapshot` 带 `AgentId` 字段，Single 模式退化为 `"main"`，为 swarm 留位 |

## 新增 crate

| crate | 职责 |
|---|---|
| `packages/agent-wire` | `Call`/`Resp`/`Frame` codec + `WsConn` 抽象 + `WireClient`（client 侧）+ `serve_ws`（server 侧），worker/CLI/控制面共享 |
| `apps/server` | `aemeath serve` 入口、`WsProxy`、`SessionManager`、`WorkerLauncher`/`LocalProcessLauncher` |

## 架构边界约定（前瞻）

| scope | 实体 | 归属 |
|---|---|---|
| session 级 | 对话/Turn/Agent Loop/workspace | worker + session 存储 |
| 账户/项目级 | Requirement/Project/Task（PM）/团队 | 独立"协作域"BC（新服务，自有 DB） |
| 基础设施级 | session 注册表/worker 调度/配额 | 控制面 |

## 非目标（defer）

认证/多租户隔离、中心 DB、真沙箱（容器/microVM）、跨机/控制面 HA、资源治理、swarm 均 defer 到后续子项目。
