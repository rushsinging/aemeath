# #36 多 Agent 框架 — Plan（实施计划）

本计划按 Sprint 从最小可用闭环逐步扩展到多 Agent 编排与自我进化。总体节奏对应原 spec 的 MVP 分层：v0.1 单 Agent + 白板最小闭环 → v0.2 多 Agent 编排 → v0.3 Evolver + RAG。

## Sprint 划分

### Sprint 0 — 基础设施

**目标**：可编译、可运行的空架子  
**依赖**：无

**产出**：
1. `share/` 目录：proto 编译脚本、common.proto（含 WatchRequest、Empty、AgentType、IdempotencyOptions）
2. `server/` 骨架：axum 启动、gRPC server 启动、CORS 配置
3. `infra/mongodb/`：collection 创建脚本（workspaces / chats / chat_messages / requirements / projects / project_tasks / agent_instances）、索引定义
4. `infra/deploy/`：docker-compose.dev.yaml（本地开发全栈：Server + MongoDB + Gateway），Dockerfile（开发镜像）
5. `agent_registry/` + `scheduler/` + `executor/` + `chat/` + `assistant/` 各 crate 骨架
6. `share/openapi/sdk/ts` SDK 目录（空壳 + package.json）
7. TOML 配置装配器（assembler.rs）：能从 5 个 toml 文件的 [role]/[permissions] 段解析 RoleConfig
8. 日志基建：`aemeath.log` + `panic.log` + `agent.log`

**涉及模块**：share/proto, server, infra, agent_registry, 所有 agent crate 骨架, config

**验证标准**：工作区能完成一次全量编译，docker-compose up 启动 Server + MongoDB；Mongo 初始化脚本可重复执行且不会破坏既有 collection/index。

---

### Sprint 1 — Server 骨架 + 对话 API

**目标**：用户能通过 API 创建 Workspace / Chat，发消息，收到 WS 推送  
**依赖**：S0

**产出**：
1. Workspace CRUD（REST：POST/GET/DELETE /api/workspaces）
2. Chat CRUD（REST：POST/GET/DELETE /api/workspaces/:ws_id/chats，一个 Workspace 下可多个 Chat）
3. ChatMessage（REST：POST /api/workspaces/:ws_id/chats/:chat_id/messages + WebSocket 推送）
4. WebSocket `/ws/workspaces/:ws_id/board`：连接后推送该 workspace 的全量 BoardSnapshot，后续增量推送 BoardSnapshotUpdate
5. Chats Service gRPC（ChatService.Create/AddMessage/Get/List/Watch）
6. 幂等：AddMessage 按 idempotency_key 去重
7. `infra/gateway/nginx.conf`：反向代理（/api → Server REST，/ws → WebSocket，/grpc → gRPC）

**涉及模块**：server（REST + WS handler）、share/proto（chat.proto）、Chat Agent（gRPC client 端）、infra/gateway

**验证标准**：通过 REST 创建 Workspace/Chat 并发送消息后，WebSocket 客户端能收到全量快照与新增消息更新；重复提交相同 idempotency_key 只产生一条 ChatMessage。

---

### Sprint 2 — UI 核心

**目标**：用户在浏览器能看到 Chat 界面，能创建 workspace、发消息、实时收到回复  
**依赖**：S1

**产出**：
1. Vue 3 + Element Plus + Pinia + Vite 项目搭建
2. `share/openapi/sdk/ts` SDK 封装：REST 客户端 + WebSocket 客户端（含自动重连和 idempotency_key 自动生成）
3. Workspace 列表页 + 创建/切换
4. Chat 页面：消息列表（滚动加载）+ 输入框 + 发送
5. WebSocket 实时消息推送渲染
6. 消息类型标识展示（requirement / feedback / chitchat）
7. 基本错误处理和重试 UI

**涉及模块**：ui/

**验证标准**：浏览器端可完成创建 workspace、进入 chat、发送消息、刷新后恢复消息列表；断开 WebSocket 后 UI 显示重连状态并能在恢复连接后重新拉取最新快照。

---

### Sprint 3 — 需求分析

**目标**：用户发需求后，Assistant 分析生成草案，用户确认后创建 Requirement  
**依赖**：S1 + S2（Server API 就绪后 UI 可联调）

**产出（后端）**：
1. Requirement CRUD（gRPC：RequirementService.Create/Update/Analyze/Confirm）
2. Requirement 状态机实现（pending → analyzing → draft → confirmed）
3. Scheduler 实现：Watch Project + Requirement，根据状态分派
4. Assistant 实现：分析 Requirement 方向（新建 Project / 已有 Project 下新增 Task / 重复无需修改），产出草案，不拆具体 Project/Task
5. Scheduler → Assistant 调度链路（Pool 管理 + 心跳检测）
6. 幂等：AnalyzeRequirement 原子抢占，Confirm 按 idempotency_key 去重

**产出（前端）**：
7. Requirement 卡片组件：显示状态（分析中/草案已产出/已确认）
8. 草案确认交互：用户查看草案、确认/修改/拒绝

**涉及模块**：scheduler/、assistant/、server（RequirementService gRPC）、ui/

**验证标准**：提交 requirement 后能观察到 pending → analyzing → draft 的状态变化，用户确认后只创建一次 confirmed Requirement；Assistant/Scheduler 任一方重启后不会重复分析已被抢占的 Requirement。

---

### Sprint 4 — 任务执行

**目标**：确认需求后，Executor 编排 Sub-Agent 执行任务，产出结果  
**依赖**：S3

**产出（后端）**：
1. Project + ProjectTask CRUD（gRPC：ProjectService.Create/Assign + TaskService.Create/Complete）
2. Project/ProjectTask 状态机实现（含取消、重试、崩溃恢复）
3. Executor 实现：接收 Project → 按 Task 顺序/并行编排 → 唤起 Sub-Agent（Planner/Coder/Reviewer/Tester/Designer）→ 收集产出 → 产出 ProjectResult.summary
4. Sub-Agent 编排：每个 Task 对应一个 Sub-Agent 角色，Executor 按 allowed_tools + context_window 管理上下文
5. Agent 间通信：Executor → Sub-Agent gRPC 调用
6. Context 上下文管理：每个 Sub-Agent 独立 context_window，超限时触发压缩
7. BoardSnapshot 增量推送：Executor 每完成一个 Task 写回 result，Server 推送增量

**产出（前端）**：
8. Project/Task 进度面板：树形结构展示 Project → Tasks → Sub-Agent 状态
9. Task 详情：Sub-Agent 日志片段、产出摘要
10. 取消/重试操作

**涉及模块**：executor/、scheduler/（Executor Pool 管理）、planner/coder/reviewer/tester/designer（Sub-Agent）、server、ui/

**验证标准**：确认草案后能生成 Project/Task 并被 Executor 领取执行，Task 完成时 UI 实时更新结果；模拟 Executor 崩溃后，Scheduler 能释放 Project 并将非终态 Task 回退到 pending 重新分配。

---

### Sprint 5+ — 优化与增强

**目标**：反思、安全、性能
**依赖**：S4

**产出**：
1. Evolver 实现（RAG 反思 + 定时巡检）
2. RBAC：Token scope 精确到 board_read/board_write/message_read/message_write，中间件校验
3. Sub-Agent 安全审计：task.log 记录 action_history
4. BoardSnapshot 性能：增量推送 + 分级订阅（Chat 全量，UI 按区域）
5. Token 刷新：execution_token 过期前的自动续期

**涉及模块**：evolver/、server（RBAC 中间件）、ui/

**验证标准**：Evolver 能定时扫描已完成 Project 并写入反思结果；RBAC 测试能证明无权限 token 无法读写对应资源，BoardSnapshot 高变更场景下仍能稳定增量推送。

---

## TOML 配置示例

### `chat.toml`

```toml
# chat.toml（面向用户的对话 Agent）
[role]
name = "chat"
description = "面向用户的对话 Agent"
system_prompt = "与用户对话，理解需求并协调调度。"
pool_size = 0               # 随连绑定，无 Pool

[[models]]
model = "anthropic/claude-sonnet-4-20250514"
cost_tier = "high"

[permissions]
allowed_tools = ["read", "write", "web_search", "web_fetch"]
scope = ["board_read", "board_write"]
can_call_roles = ["scheduler"]  # Chat 只能调 Scheduler
max_subagents = 0              # Chat 不唤起 Sub-Agent
```

### `assistant.toml`

```toml
# assistant.toml（后台需求分析/草案 Worker）
[role]
name = "assistant"
description = "后台需求分析/草案 Worker"
system_prompt = "分析用户需求并生成可确认草案。"
pool_size = 3               # Scheduler 管理 Pool

[[models]]
model = "deepseek/deepseek-chat"
cost_tier = "low"

[permissions]
allowed_tools = ["read", "write", "grep", "glob"]
scope = ["board_read", "board_write"]
can_call_roles = []
max_subagents = 0
```

### `scheduler.toml`

```toml
# scheduler.toml
[role]
name = "scheduler"
description = "管理 Agent Pool 生命周期，分派任务"
system_prompt = "管理 Agent Pool 生命周期并分派任务。"
pool_size = 1
[[models]]
model = "deepseek/deepseek-chat"
cost_tier = "low"

[permissions]
allowed_tools = []
scope = ["agent_registry", "board_read", "board_write"]
can_create_agents = true
can_call_roles = []
max_subagents = 0

[skills]
enabled = []

[mcp]
servers = []
```

### `executor.toml`

```toml
# executor.toml
[role]
name = "executor"
description = "领取 Project，编排 Sub-Agent 执行 Tasks，写回白板"
system_prompt = "领取项目并编排 Sub-Agent 执行任务。"

[[models]]
model = "anthropic/claude-sonnet-4-20250514"
cost_tier = "high"

[[models]]
model = "openai/gpt-5"
cost_tier = "high"

[[models]]
model = "deepseek/deepseek-v4-pro"
cost_tier = "medium"

[permissions]
allowed_tools = ["agent_call"]
# agent_call 是 allowed_tools（runtime 工具），不属于 scope
scope = ["board_read", "board_write"]
max_subagents = 5
can_call_roles = ["planner", "coder", "tester", "reviewer", "designer"]
can_create_agents = false

[skills]
enabled = ["task-management"]

[mcp]
servers = []
```

### `evolver.toml`

```toml
# evolver.toml
[role]
name = "evolver"
description = "定期扫描白板，提炼模式，生成/优化 Skills 和 MCP"
system_prompt = "定期扫描白板并提炼可复用经验。"

[[models]]
model = "deepseek/deepseek-chat"
cost_tier = "low"

[permissions]
allowed_tools = ["web_search"]
scope = ["board_read", "board_write"]
max_subagents = 0
can_call_roles = []
can_create_agents = false

[skills]
enabled = ["analysis", "summarization"]

[mcp]
servers = []
```

## P0 设计约束

### 故障恢复

#### Executor 崩溃恢复

```text
1. Scheduler 心跳超时检测（heartbeat_timeout_sec, 默认 30s）
2. 超时 Executor 的 current_project_id → 查 Project status:
   - in_progress → 仅在崩溃恢复路径清空 assigned_executor_id，并将 Project 回退 pending 以便重新分配
   - blocked     → Scheduler 通知 Chat，等待用户决策
   - completed/failed/cancelled → 终态不回退
3. Project 崩溃恢复条件更新（防并发）:
   db.projects.updateOne(
     { _id: project_id, status: { $in: ["pending", "assigned", "in_progress"] }, assigned_executor_id: old_id },
     { $unset: { assigned_executor_id: "" }, $set: { status: "pending" } }
   )
4. 级联回退该 Project 下由该 Executor 持有的非终态 Task：
   InProgress / InReview / Retrying → Pending（清空 assigned_executor_id）
5. 重新分配给新 Executor 时写入原因字段（如 "reassigned_after_crash"）

约束：普通执行失败不会自动 Failed → Pending；只有 Executor 崩溃恢复回退非终态 Task，或显式人工重试/重开才会重新进入 Pending/分配流程。
```

#### Task 重试

```text
ProjectTask 状态:
  pending → in_progress → in_review → (completed | failed | retrying)
  in_progress → retrying → in_progress（下一次 attempt 开始，形成明确重试循环）
  in_progress / in_review / retrying → pending（仅 Executor 崩溃恢复回退；普通失败不走此路径）

Executor 重启后:
  1. 查询自身 assigned_task（status=in_progress 或 retrying）
  2. 从 Mongo 加载 Task 上下文（description + related_message_ids）
  3. 重新执行
  4. Sub-Agent 返回 retry_needed → 不写 final result；写一条 ProjectTaskResult(status=retry_needed) intermediate attempt result，ProjectTask.status=retrying，然后重新执行
  5. 连续失败或 retry_needed 超过 max_task_retries（默认 3）→ 写 ProjectTaskResult(status=failed) final result → ProjectTask.status=failed → 通知 Chat

重试携带 retry_count + last_error，Sub-Agent 可根据失败历史调整策略。
```

#### Watch 断线恢复

```text
v0.1 Watch 定位：实时提示（best-effort），不是可靠消息队列。

关键约束:
  - API Server 不为 subscriber 缓冲事件。断线期间事件丢失。
  - Scheduler 重启 / gRPC Watch 断线后必须全量扫描 assigned 超时、in_progress 且 assigned executor 心跳超时、以及 busy Executor 但 current_project_id 不存在或已终态的异常绑定。
  - resume_token 由客户端保存，用于减少断线后重复消费，但不保证不丢事件。

重连策略:
  - 指数退避重连：1s → 2s → 4s → ... → max 60s
  - 重连后：检查 checkpoint 时间 → 判断是否需全量扫描
  - 全量扫描频率上限：每 60s 最多 1 次（防止雪崩）

白板 WebSocket 断线:
  - UI 显示连接状态（disconnected / reconnecting / connected）
  - 重连后：REST GET /board/{workspace_id} 全量拉取最新快照，覆盖当前 state
```

### 幂等策略

- 所有会产生写入副作用的 gRPC/REST 请求必须携带 `idempotency_key`（可通过 `IdempotencyOptions` 表达），由 Server 在目标资源范围内去重。
- `ChatService.AddMessage`：按 `workspace_id + chat_id + idempotency_key` 去重，重复请求返回第一次创建的 ChatMessage。
- `RequirementService.Analyze`：使用条件更新完成原子抢占，仅允许 `pending` Requirement 进入 `analyzing`，避免多个 Assistant 同时分析同一需求。
- `RequirementService.Confirm`：按 `requirement_id + idempotency_key` 去重，避免重复创建 Project/Task。
- Project/Task 状态更新使用条件更新（例如校验 `assigned_executor_id`、当前 status、attempt），避免崩溃恢复、重试、并发 Executor 写回互相覆盖。
- Watch 的 `resume_token` 只用于断线后减少重复消费，不承诺可靠队列语义；断线恢复后的最终一致性依赖全量扫描/快照覆盖。

## 开放问题

- MongoDB 驱动：**已定稿** — 使用官方 mongodb crate。
- Scheduler 实例数：**已定稿** — v0.1 严格单例；v0.2+ 可在实现 Executor/Assistant Pool 后再评估多实例选主/分片调度。
- Embedding 模型：**已定稿** — Sprint 0~4 暂不实现；Sprint 5+ 的 Evolver/RAG 阶段再选型并接入。
- 前端技术选型：**已定稿** — Vue 3 + Element Plus + Pinia + Vite，数据获取走 `share/openapi/sdk/ts`。
- Qdrant / RAG：**延期到 Sprint 5+** — 本地开发未配置 Qdrant 时禁用 RAG，只保留规则化总结。
- CLI 集成：**延期** — 本计划优先 server/ui/agents 闭环，CLI 作为 API Server 客户端在核心闭环后再接入。
