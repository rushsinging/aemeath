# #36 Multi-Agent 框架设计

## 概述

将当前单 Agent 架构升级为多 Agent 协作框架，参考 K8s 控制面设计：
- **API Server**（数据面）— gRPC（Agent 间通信）+ REST/WebSocket（前端），白板数据 CRUD + Watch
- **Scheduler**（调度面）— 管理 Agent Pool 生命周期，按需求量动态扩缩
- **Agent 角色**— 5 类 Main Agent（Chat / Scheduler / Executor / Assistant / Evolver）+ Sub-Agent（Executor 唤起，无状态）
- **对话/分析拆解**— Assistant（由 Scheduler 调度）负责分析用户消息类型、拆解需求为 Project/Task，草案存入 `Requirement.draft`；Chat 面向用户多轮对话并汇报状态
- **白板**（呈现层）— 纯 UI，通过 REST/WS 调 Server 获取数据渲染
- **MVP 分级交付**— v0.1 单 Agent+白板 → v0.2 多 Agent 编排 → v0.3 自我进化
- **P0 设计约束**— Executor 崩溃恢复、Task 重试、幂等性、Watch 断线恢复

## 核心决策

| 维度 | 决策 |
|---|---|
| 通信模式 | 白板 SSOT，所有 Agent 通过 API Server 间接通信 |
| API 协议 | tonic gRPC（Agent 间）+ REST/WebSocket（前端） |
| Watch 机制 | API Server 独占 MongoDB Change Streams；Agent / 前端通过 gRPC Server Streaming / WebSocket 间接消费 |
| 数据库 | MongoDB（文档型，无外键，数组存引用） |
| 调度层次 | 两层：Scheduler → Executor → Sub-Agent |
| 租户隔离 | Workspace 归属租户（个人/团队） |
| Session 策略 | 5 类 Main Agent（Chat / Scheduler / Executor / Assistant / Evolver）各自管理上下文；Sub-Agent 无状态，上下文由 Executor gRPC 传递 |
| 白板访问 | Chat / Scheduler / Executor / Assistant / Evolver 可访问；Sub-Agent 不可访问 |
| Agent 实现 | 一个通用模板 + 装配器（role → skill/MCP/prompt/权限），角色配置由 TOML 定义，支持用户自定义 Role |
| 需求分析 | Chat 接收用户消息，Scheduler 调度 Assistant 分析消息类型、拆解需求 → Project/Task，草案存入 `Requirement.draft`，用户确认后写入 Project/Task |
| Executor 分配策略 | Executor 按 Project 独占绑定，一个 Project 同时只分配一个 Executor |
| Evolver | 独立后台进程，定期扫描白板：总结已完成项目 → 提炼可复用模式 → 生成/优化 Skills、MCP 配置，驱动系统自我进化 |
| P0 约束 | MongoDB replica set（Change Streams 必需）；Change Streams 由 API Server 独占订阅，Agent / 前端通过 gRPC stream / WebSocket 间接消费 |
| MVP 交付 | v0.1 单 Agent + 白板 → v0.2 多 Agent 编排 → v0.3 自我进化（Evolver + RAG） |
| P0 故障恢复 | Executor 崩溃 → 心跳超时释放 Project，并仅在崩溃恢复时将非终态 Task（InProgress/InReview/Retrying）回退 Pending；Task 重试 ≤3 次；gRPC 幂等写（idempotency_key）；Watch resume_token 断线续传 |

## 数据流

```
用户
 │
 ▼
Chat ──(写用户需求消息)──▶  API Server ──▶  Mongo
 │                                       │
 │                                       ▼
 │                                 Render ◀── Mongo
 │                                       │
 │                                       ▼
 │                              白板（ChatMessage 展示）
 │
 │ 委托 Scheduler 调度 Assistant 分析消息类型、拆解需求（草案写入 Requirement.draft，待用户确认）
 │ 确认后写入 ▼
API Server ──▶ Mongo ──▶ Render ──▶ 白板（Project + Project Task）

Scheduler ──Watch Project──▶ API Server
    │
    │ 管理 Pool，按 Project 分配 Executor；调度 Assistant 分析 Requirement
       ▼
     Executor ──▶  Sub-Agent（Planner / Coder / Tester / Reviewer / Designer）
       │               ↑
       │               │ 子 Agent 不访问白板，上下文由 Executor gRPC 传递
       │
       │ 写回 Project/Task 状态 → 白板变更
       │ Chat Watch 白板 → 感知状态变化 → 汇报用户
       ▼
     API Server ──▶ Mongo ──▶ Render ──▶ 白板（状态更新）

Evolver（独立后台）──定期扫描白板──▶ API Server
     │
     │ 分析已完成 Project，提炼模式
     │ 生成 / 优化 Skills、MCP
     ▼
  API Server ──▶ Mongo（写入 Skill / MCP 配置）
```

## 架构概览

```
                    白板 UI（Vue + Element Plus，ui/）
                         │  REST / WebSocket
                         ▼
┌──────────────────────────────────────────────────────────┐
│              API Server（server/）                          │
│                                                            │
│  ┌──────────────────┐   ┌──────────────────────────┐      │
│  │  REST / WS 网关   │   │   gRPC Service（Agent 间）│      │
│  │  （前端接口）      │   │                          │      │
│  └────────┬─────────┘   └──────────┬───────────────┘      │
│           │                        │                       │
│  ┌────────┴────────────────────────┴──────────────────┐    │
│  │ Chat Svc  │ Req Svc │ Proj Svc │ Task Svc │ Agent Reg │    │
│  └────────────────────────────────────────────────────┘    │
│                          │                                  │
│                    MongoDB（文档存储）                       │
└──────────────────────────────────────────────────────────┘
        ▲                      ▲
        │ gRPC Watch           │ gRPC Watch / CRUD
        │                      │
┌───────┴──────┐          ┌────┴──────────────────┐
│  Scheduler   │          │  Chat（随连绑定）       │
│  （单例）     │          │  接收用户消息            │
│              │          │  写 ChatMessage         │
│ Watch        │          │  Watch BoardSnapshot   │
│ Project      │          │  Watch Requirement     │
│              │          │  汇报用户              │
│              │          └───────────────────────┘
└──────┬───────┘
       │ 创建/调度 Assistant Pool 执行 Requirement 分析
       ▼
┌─────────────────────┐
│  Assistant Pool     │
│  后台分析/拆解/汇总   │
└─────────────────────┘
       │ 分派 Project (gRPC)
       ▼
┌─────────────────────┐
│  Executor Pool      │
│  #1, #2, ...        │
│  （持有 Session，   │
│   访问白板）         │
└──────────┬──────────┘
           │ 唤起 Sub-Agent（进程内调用，传递精简上下文）
           ▼
     ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐
     │ Planner  │  │  Coder   │  │  Tester  │  │ Reviewer │  │ Designer │
     │ (无状态)  │  │ (无状态)  │  │ (无状态)  │  │ (无状态)  │  │ (无状态)  │
     └──────────┘  └──────────┘  └──────────┘  └──────────┘  └──────────┘

     ┌───────────────────────────────────┐
     │  Evolver（独立后台，单例）         │
     │  定期扫描白板 → 提炼模式           │
     │  生成 / 优化 Skills、MCP          │
     └───────────────────────────────────┘
```

## Session 与白板访问权限

### 角色分类

| 类型 | Agent | Session | 白板访问 | 说明 |
|---|---|---|---|---|
| Main Agent | Chat | 有 | 有 | 面向用户多轮对话，接收用户消息，Watch BoardSnapshot 汇报用户 |
| Main Agent | Scheduler | 无（控制循环） | 有 | Watch + 分配决策，管理 Executor/Assistant Pool，不参与对话 |
| Main Agent | Executor | 有 | 有 | 持有 Project 执行上下文，编排 Sub-Agent 执行 Tasks，有意义问题反馈 Chat（由 Chat 汇报用户） |
| Main Agent | Assistant | 有 | 有 | 后台 worker，分析需求、拆解 Project/Task、产出草案、结果汇总，Pool 由 Scheduler 调度 |
| Main Agent | Evolver | 无（定时循环） | 有 | 独立后台，定期扫描已完成 Project 和 Executor 记录，提炼可复用模式，生成/优化 Skills、MCP 配置 |
| Sub-Agent | Planner | 无 | 无 | 一次性：接收需求 → 返回计划 |
| Sub-Agent | Coder | 无 | 无 | 一次性：接收 spec → 返回代码 |
| Sub-Agent | Tester | 无 | 无 | 一次性：接收代码 → 返回测试结果 |
| Sub-Agent | Reviewer | 无 | 无 | 一次性：接收 PR → 返回 review 意见 |
| Sub-Agent | Designer | 无 | 无 | 一次性：接收描述 → 返回设计稿 |

### 上下文传递

```
白板（持久上下文，Mongo）──▶ Executor 读取 ──▶ 精简摘要（gRPC 请求体）──▶ Sub-Agent 执行
                                                              │
                                                              ▼
                                                        结果（gRPC 响应）
                                                              │
                                                              ▼
                              Executor 写回 ◀── 白板（持久化）
```

Sub-Agent **不感知白板存在**。Executor 是上下文的翻译层：持久上下文 → 工作摘要 → Sub-Agent → 收集结果 → 写回白板。

### 权限校验

| 校验层 | 位置 | 规则 |
|---|---|---|
| Board 范围 | API Server gRPC 中间件 | Chat/Executor/Assistant/Evolver 的 token `scope` 包含 `board_read`/`board_write`；Main Agent 请求被 gRPC 中间件拦截（token 中携带 role/scope）；Sub-Agent 不直接连 API Server |
| Tool allowlist | Agent 装配时注入 | 按 RoleConfig.permissions 的 `allowed_tools` 过滤运行时工具（Bash/Read/Write/WebSearch/Grep/Glob 等） |
| Sub-Agent 调用 | Executor 端校验 | 按 RoleConfig.permissions 的 `can_call_roles` 限制可选角色 |
| 凭据隔离 | 装配时注入 | Sub-Agent 无 board 访问 token；Executor 不传递自身 credential |

权限分层：
- `scope` 是 API Server 资源权限，用于 gRPC/REST 中间件鉴权，例如 `board_read`、`board_write`、`agent_registry`。
- `allowed_tools` 是 Agent runtime 可调用的工具白名单，例如 `Bash`、`Read`、`Write`、`WebSearch`、`Grep`、`Glob`。
- `board_read` / `board_write` 不属于 `allowed_tools`，只能出现在 token `scope` 或角色的 API 资源权限说明中。

## 白板渲染区域

| 区域 | 数据源 | 说明 |
|---|---|---|
| 用户需求消息 | Requirement | 用户通过 Chat 提交的需求记录（1:1 关联 ChatMessage） |
| 草案 | Requirement.draft | Assistant（由 Scheduler 调度）拆解产出的 Project/Task 草案 |
| Project & Task | Project + ProjectTask | 需求拆解后的项目与任务 |
| Agent 状态 | Agent Registry | 当前活跃的 Agent 实例、状态 |
| 自定义数据区块 | 扩展注册 | 支持新增其他数据类型渲染 |

## 数据库核心实体（MongoDB 文档）

所有关联通过文档内数组存引用 ID，不使用外键约束。

### Workspace
```jsonc
{
  "_id": ObjectId,
  "tenant_id": ObjectId,
  "name": "我的工作空间",
  "created_at": ISODate
}
```

### Chat（用户会话）
```jsonc
{
  "_id": ObjectId,
  "workspace_id": ObjectId,
  "title": "讨论登录页重构",
  "status": "active",           // active | archived
  "created_at": ISODate,
  "updated_at": ISODate,
  "version": 0,                  // u64，乐观锁，每次更新 +1
}
```

### ChatMessage（会话消息）
```jsonc
{
  "_id": ObjectId,
  "chat_id": ObjectId,
  "workspace_id": ObjectId,
  "role": "user",               // user | chat
  "content": "帮我做一个登录页面...",
  /*
   * message_type 由 Chat 分析后写入：
   *   question     - 简单提问（不需要拆解）
   *   requirement  - 用户需求标记，1:1 关联 Requirement 文档
   *   clarification - 澄清/追问
   *   feedback     - 反馈/确认
   */
  "message_type": "requirement",
  "requirement_id": ObjectId,    // 可选；仅 requirement 消息有，1:1 关联 Requirement
  "metadata": {},                // 扩展字段
  "embedding_ref": {
    "collection": "chat_messages",
    "point_id": "<message_object_id>"
  },                              // Qdrant 引用（message_type=requirement 时有）
  "embedding_status": "pending", // pending | indexed | failed
  "created_at": ISODate,
  "version": 0,                  // u64，乐观锁，每次更新 +1
}
```

### Requirement
```jsonc
{
  "_id": ObjectId,
  "workspace_id": ObjectId,
  "source_message_id": ObjectId,     // 1:1 关联 ChatMessage
  "title": "登录页面重构",
  "description": "需要重新设计登录页面...",
  "category": "raw",                 // raw | organized
  "status": "pending",               // pending | analyzing | draft | in_progress | completed | rejected | cancelled
  "version": 0,
  "project_ids": [ObjectId],         // 关联的 Project（N:N）
  "task_ids": [ObjectId],            // 关联的 ProjectTask（N:N，完成判定）
  "draft": {
    "projects": [ { "name": "...", "tasks": [...] } ],
    "summary": "...",
    "created_by": "chat_agent_id"
  },
  "draft_history": [ { "revision": 0, "draft": {...}, "timestamp": ISODate } ],
  "created_at": ISODate,
  "updated_at": ISODate
}
```

### Project
```jsonc
{
  "_id": ObjectId,
  "workspace_id": ObjectId,
  "requirement_ids": [ObjectId], // 关联的 Requirement（N:N）
  /*
   * assigned_executor_id — 当前分配的 Executor Agent ID
   * 可选；仅 assigned 后有值
   * 独占保证：Scheduler 事务 + AgentInstance.current_project_id 部分唯一索引，详见 Scheduler 独占分配机制
   */
  "assigned_executor_id": ObjectId,
  "name": "登录页 UI 重构",
  "status": "pending",           // pending | assigned | in_progress | blocked | failed | completed | cancelled
  "version": 0,                  // 乐观锁
  "assigned_at": ISODate,        // 分配给 Executor 的时间；assigned 状态下用于超时检测
  "assignment_attempts": 0,      // 分配尝试次数；每次分配递增
  "merge_lock": {                // git merge 锁（同一 main 分支串行 merge）；创建 Project 时必须初始化为 { locked_by: null, locked_by_executor: null, locked_at: null }
    "locked_by": "task_xxx",     //   当前持有锁的 ProjectTask ID；null = 未锁定（兼容锁获取条件）
    "locked_at": ISODate,        //   锁获取时间
    "locked_by_executor": "exec-1" // 持有锁的 Executor ID
  },
  "summary": "",                 // Executor 完成后写入的项目总结
  "key_decisions": [],           // 关键决策列表
  "embedding_ref": {
    "collection": "projects",
    "point_id": "<project_object_id>"
  },                              // Qdrant 引用（status=completed 时有）
  "embedding_status": "pending", // pending | indexed | failed
  "reflected": false,            // Evolver 是否已反思
  "created_at": ISODate,
  "updated_at": ISODate
}
```

```javascript
db.projects.createIndex(
  { "merge_lock.locked_by_executor": 1 },
  {
    partialFilterExpression: {
      "merge_lock.locked_by_executor": { $exists: true }
    }
  }
)
```

### ProjectTask
```jsonc
{
  "_id": ObjectId,
  "project_id": ObjectId,
  "workspace_id": ObjectId,
  "title": "实现表单验证",
  "description": "需要支持邮箱格式校验...",
  "status": "pending",           // pending | in_progress | in_review | completed | failed | retrying | cancelled
  "version": 0,                  // 乐观锁
  "assigned_executor_id": ObjectId, // 执行该 Task 的 Executor instance ID（不是 Sub-Agent）
  "executor_type": "coder",                 // 执行此 Task 需要的角色类型（planner / coder / tester / reviewer / designer）
  "depends_on": [ObjectId],            // 前置 Task ID 列表
  "depends_type": "all",               // all（全部完成）/ any（任一完成）
  "priority": 1,
  /*
   * related_message_ids — 关联的 ChatMessage
   * 执行过程中产生的上下文消息（如：Executor 提问、Chat 回复）
   */
  "related_message_ids": [ObjectId],
  "output_summary": "",            // Executor 完成后写入的产出摘要 + 遇到的坑/解法
  "embedding_ref": {
    "collection": "project_tasks",
    "point_id": "<task_object_id>"
  },                                // Qdrant 引用（status=completed 时有）
  "embedding_status": "pending",   // pending | indexed | failed
  "created_at": ISODate,
  "updated_at": ISODate
}
```

### ProjectTaskResult（Executor 执行产出）
```jsonc
{
  "_id": ObjectId,
  "project_task_id": ObjectId,        // 关联的 ProjectTask
  "executor_id": ObjectId,            // 执行者 AgentInstance._id
  "status": "completed",              // completed | failed | retry_needed
  "output": "Sub-Agent 产出的代码...",  // 执行输出；retry_needed 时为本次尝试的中间输出/诊断
  "summary": "LLM 摘要的任务执行小结",
  "artifacts": ["file_path", "snippet_id"],
  "error_message": null,              // 失败原因（如有）；retry_needed 时记录需要重试的原因
  "retry_count": 0,                   // 重试次数（当前 attempt 对应的 retry_count）
  "is_final": true,                   // true=最终结果（completed/failed）；false=中间 attempt（retry_needed）
  "created_at": ISODate
}
```

```javascript
db.project_task_results.createIndex({ project_task_id: 1 })
```

### ProjectResult（Executor 执行完后对 Project 的整体产出）
```jsonc
{
  "_id": ObjectId,
  "project_id": ObjectId,
  "executor_id": ObjectId,
  "summary": "所有 Task 完成后 Executor 总结的项目产出",
  "key_decisions": ["采用 React 18...", "API 用 REST..."],
  "task_results_ref": [ObjectId],      // 关联最终 ProjectTaskResult._id（仅 is_final=true；不包含 status=retry_needed 的 intermediate attempt result）
  "created_at": ISODate,
  "updated_at": ISODate
}
```

### AgentInstance
```jsonc
{
  "_id": ObjectId,
  "workspace_id": ObjectId,
  "role": "executor",            // 角色标识（内置 + 用户自定义）
  "role_config_ref": "roles/executor.toml",
  "status": "idle",              // initializing | idle | busy | error
  "version": 0,                  // 乐观锁
  "active_model": "anthropic/claude-sonnet-4-20250514",
  "model_state": {
    "models": [
      { "model": "anthropic/claude-sonnet-4-20250514", "status": "healthy" },
      { "model": "openai/gpt-5-codex", "status": "healthy" }
    ]
  },
  "current_project_id": ObjectId, // 当前处理的 Project（Executor 专用）
  "last_heartbeat": ISODate,
  "created_at": ISODate
}
```

### BoardSnapshot（白板视图，不持久化）

> **Canonical 定义见 [Board 聚合响应结构](#board-聚合响应结构)（REST API 中 Rust struct）。**
>
> BoardSnapshot 不存 MongoDB，由 `board.rs` 实时计算。仅 REST API struct 为唯一定义，此处不重复。

### agent_heartbeats（Agent 心跳记录）

```jsonc
{
  "_id": ObjectId,
  "agent_id": ObjectId,
  "workspace_id": ObjectId,
  "role": "executor",
  "heartbeat_at": ISODate,
  "current_project_id": ObjectId        // 仅 Executor
}
// 索引: { agent_id: 1, heartbeat_at: -1 }
// TTL: heartbeat_at 过期 120s 后自动删除（2 × heartbeat_timeout_sec）
// 说明：agent_heartbeats 仅记录 Executor 心跳；Scheduler 对账 checkpoint 不写入此 collection。
```

### scheduler_state（Scheduler 状态 checkpoint）

```jsonc
{
  "_id": "scheduler_state",             // 单文档
  "checkpoint_time": ISODate,           // 最近一次对账 checkpoint
  "last_full_scan": ISODate,            // 最近一次全量扫描时间
  "processed_count": 0                  // 最近一次对账处理数量
}
```

### idempotency_records（幂等记录）

```jsonc
{
  "_id": "uuid-string",                 // 幂等键
  "endpoint": "AssignProject",          // gRPC 方法名
  "response": { "project_id": "...", "status": "assigned" },  // 缓存的响应
  // 最大 4KB，超出截断
  "created_at": ISODate
}
// TTL: created_at 过期 24h 后自动删除
```

### reflections（Evolver 反思记录）

```jsonc
{
  "_id": ObjectId,
  "workspace_id": ObjectId,
  "project_id": ObjectId,
  "iteration": 1,                       // 反思轮次
  "analysis": "当前项目中 Template 代码重复度高，建议抽取共享按钮组件",
  "suggested_changes": [
    {
      "type": "code_pattern",           // code_pattern | role_config | skill_suggestion
      "target": "Button component",
      "recommendation": "创建 shared/ui/Button.astro"
    }
  ],
  "applied": false,                     // 用户是否已采纳
  "applied_at": null,
  "embedding_ref": {
    "collection": "reflections",
    "point_id": "<reflection_object_id>"
  },
  "embedding_status": "pending",        // pending | indexed | failed
  "created_at": ISODate
}
```

### model_health（v0.2，v0.1 仅用 AgentInstance.model_state）

v0.1 简化方案：`model_health` 不在 MVP 范围内，模型状态仅由 `AgentInstance.model_state` 维护。
v0.2 由 API Server 聚合所有 Agent 的 model_state 形成全局 model_health collection。

```jsonc
// v0.2:
{
  "_id": ObjectId,
  "model": "anthropic/claude-sonnet-4-20250514",
  "status": "healthy",                  // healthy | degraded | unhealthy
  "failed_requests": 0,
  "total_requests": 0,
  "error_rate": 0,
  "avg_latency_ms": 0,
  "last_checked": ISODate
}
```

### 数据关联总览（无外键，数组引用）
```
Workspace  1:N  Chat
Chat  1:N  ChatMessage
  ChatMessage  1:1  Requirement（仅 message_type=requirement）
    Requirement  N:N  Project（requirement_ids / project_ids）
    Requirement  N:N  ProjectTask（task_ids，用于完成判定）
      Project  1:N  ProjectTask
        ProjectTask  1:N  ProjectTaskResult
      Project  1:1  ProjectResult
Workspace  1:N  AgentInstance
  AgentInstance  N:1  Project（assigned_executor_id，独占）
```

## 关键数据结构

### MessageType（ChatMessage 类型枚举）
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MessageType {
    Question,        // 简单提问，不需要拆解
    Requirement,     // 用户需求消息标记，1:1 关联 Requirement 文档
    Clarification,   // 澄清/追问
    Feedback,        // 反馈/确认
}
```

### RequirementStatus（Requirement 状态枚举）
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RequirementStatus {
    Pending,        // 待分析
    Analyzing,      // Assistant 正在分析中（原子抢占，Assistant 是后台 worker）
    Draft,          // 草案已产出，等待用户确认（允许多轮 Draft→Draft）
    InProgress,     // 关联 ProjectTask 正在执行中
    Completed,      // 所有关联 ProjectTask 为 Completed 或 Cancelled
    Rejected,       // 用户驳回草案；重新提交后 → Analyzing
    Cancelled,      // 用户取消
}
```

### ProjectStatus（Project 状态枚举）
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ProjectStatus {
    Pending,        // 待 Scheduler 分配 Executor
    Assigned,       // 已分配 Executor，等待 Accept（超时 60s → Pending）
    InProgress,     // Executor 已接受并正在执行
    Blocked,        // 等待用户反馈，Agent 主动提醒用户解锁（无系统自动超时）
    Failed,         // 执行失败终态之一；普通失败不自动回退 Pending，显式人工重试/重开除外
    Completed,      // 全部 ProjectTask 完成（冻结）
    Cancelled,      // 用户 / Scheduler 终止
}
```

### ProjectTaskStatus（ProjectTask 状态枚举）
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ProjectTaskStatus {
    Pending,        // 待执行（Executor 按 DAG 调度）
    InProgress,     // 正在执行
    InReview,       // 进入 Review 阶段（Review 不通过 → InProgress 返工）
    Completed,      // 执行成功
    Failed,         // 最终失败；普通失败不自动回退 Pending，显式人工重试/重开除外
    Retrying,       // 重试中
    Cancelled,      // 用户取消 / Project 级联取消
}
```

### AgentStatus（AgentInstance 状态枚举）
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AgentStatus {
    Initializing,   // Scheduler 创建后，正在加载配置 / 建立连接
    Idle,           // 空闲，可接收新任务
    Busy,           // 正在执行任务
    Error,          // 异常状态（暂时性 → 冷却恢复 Idle；持久性 → Scheduler 销毁）
}
```

说明：Agent 销毁即删除对应 `AgentInstance` 文档，无需单独的 `Destroyed` / `Deregistered` 终态。

### AgentRole（动态角色标识，支持内置 + 用户自定义）

角色不分固定枚举，而是字符串标识 + 角色配置文件。内置角色按生命周期分为两组：

| Main Agent（长期运行） | Sub-Agent（按需唤起） |
|---|---|
| chat | - |
| assistant | planner |
| scheduler | coder |
| executor | tester |
| evolver | reviewer |
|  | designer |

Rust 侧保留字符串常量引用：

```rust
/// 内置角色常量（代码引用用）
pub mod builtin_roles {
    pub const CHAT: &str = "chat";
    pub const ASSISTANT: &str = "assistant";
    pub const SCHEDULER: &str = "scheduler";
    pub const EXECUTOR: &str = "executor";
    pub const EVOLVER: &str = "evolver";
    pub const PLANNER: &str = "planner";
    pub const CODER: &str = "coder";
    pub const TESTER: &str = "tester";
    pub const REVIEWER: &str = "reviewer";
    pub const DESIGNER: &str = "designer";
}

/// 角色配置：TOML 文件反序列化
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoleConfig {
    pub name: String,                    // 角色标识，如 "security-auditor"
    pub description: String,
    pub system_prompt: String,
    pub models: Vec<RoleModelConfig>,    // 模型列表，按优先级排列
    pub permissions: RolePermissions,     // Agent runtime 工具权限；API Server 资源权限由 token scope 控制
    pub skills: Vec<String>,
    pub mcp: McpConfig,                 // 继承现有 McpConfig 定义
    // 用户自定义角色可额外扩展字段
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RolePermissions {
    pub allowed_tools: Vec<String>,       // Agent runtime 工具白名单；不包含 board_read/board_write 等 API scope
    #[serde(default)]
    pub scope: Vec<String>,               // API Server 资源权限 scope，如 board_read/board_write/agent_registry
    #[serde(default = "default_max_subagents")]
    pub max_subagents: usize,
    #[serde(default)]
    pub can_call_roles: Vec<String>,      // 可唤起的 Sub-Agent 角色列表
    #[serde(default)]
    pub can_create_agents: bool,          // 是否允许创建 Agent（仅 scheduler 为 true）
}
```

用户自定义角色：在 `roles/` 目录下新增 TOML 文件，Scheduler 启动时扫描加载。例如：

```toml
# roles/security-auditor.toml（用户自定义）
[role]
name = "security-auditor"
description = "安全审计 Agent"
[[models]]
model = "anthropic/claude-sonnet-4-20250514"
cost_tier = "high"

[permissions]
allowed_tools = ["Grep"]
scope = ["board_read"]
max_subagents = 0
can_call_roles = []
can_create_agents = false
```

### PoolConfig（Pool 扩缩配置）
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolConfig {
    pub role: String,                // 角色标识字符串（内置常量或自定义名）
    pub min_instances: usize,       // 最小实例数
    pub max_instances: usize,       // 最大实例数
    pub scale_up_threshold: usize,  // 扩容阈值（pending 数 / 实例数）
    pub scale_down_idle_secs: u64,  // 空闲多久后缩容（秒）
    pub heartbeat_interval_secs: u64,
    pub heartbeat_timeout_secs: u64, // 心跳超时判定 dead
}
```

## 多模型支持

三层体系，自上而下：

```
Layer 1: 角色绑模型（静态配置）
   └── 每个 Agent 角色指定主模型 + 备选列表

Layer 2: 故障转移（运行时）
   └── 主模型不可用 → 自动按优先级降级到备选

Layer 3: 成本分层（任务粒度）
   └── 简单任务用便宜模型，复杂任务用强模型
```

### Layer 1：角色 → 模型绑定

RoleConfig 中配置模型列表，优先级递减：

```toml
# roles/coder.toml
[role]
name = "coder"
description = "代码实现 Agent"

[[models]]                        # 按优先级排列
model = "anthropic/claude-sonnet-4-20250514"
cost_tier = "high"

[[models]]
model = "gpt-5-codex"
cost_tier = "high"

[[models]]
model = "deepseek/deepseek-v4-pro"
cost_tier = "medium"
```

```rust
/// 角色 → 模型配置（RoleConfig 的一部分）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoleModelConfig {
    pub model: String,                    // "provider/model_id"
    pub cost_tier: CostTier,              // high | medium | low
    #[serde(default)]
    pub max_retries: u32,                  // 该模型的最大重试次数
    #[serde(default = "default_retry_delay")]
    pub retry_delay_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CostTier {
    High,       // 强模型，用于核心任务（代码生成、Review）
    Medium,     // 中等模型，用于分析、规划
    Low,        // 便宜模型，用于简单任务
}
```

`CostTier` 的 protobuf 枚举定义在 `share/proto/common.proto`，其他 proto message（如 `ExecuteTaskRequest`）通过 import common.proto 复用该枚举。

### Layer 2：故障转移

Agent 实例在执行时维护模型状态。当主模型返回可重试错误（rate limit、5xx）时自动降级：

```
请求 claude-sonnet-4  ──(503)──▶ 标记该模型不可用，尝试下一个
        │
请求 gpt-5-codex     ──(200)──▶ 正常执行
```

```rust
#[derive(Debug, Clone)]
pub struct AgentModelState {
    pub active_model: String,                    // 当前使用的模型
    pub models: Vec<ModelStatus>,                // 所有可用模型
}

#[derive(Debug, Clone)]
pub struct ModelStatus {
    pub model: String,
    pub status: ModelHealth,
    pub consecutive_failures: u32,
    pub last_failed_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ModelHealth {
    Healthy,
    Degraded,    // 失败但未达阈值，仍可尝试
    Unhealthy,   // 连续失败超阈值，暂时熔断
}
```

**熔断规则**：
- 连续失败 ≥ 3 次 → 标记 Unhealthy
- Unhealthy 后冷却 60s → 恢复为 Degraded（允许再次尝试）
- Degraded 一次成功 → 恢复 Healthy

**降级顺序**：
1. 按 RoleModelConfig 列表优先级 → 同一 cost_tier 内降级
2. 同 tier 全部 Unhealthy → 降级到下一 tier（high → medium → low）
3. 所有模型全部 Unhealthy → 触发 Scheduler 告警，任务 Pending

### Layer 3：成本分层

不同任务自动路由到不同 cost tier 的模型：

| 任务类型 | Cost Tier | 典型场景 |
|---|---|---|
| 代码生成 / 重构 / 复杂 review | High | Coder 核心任务 |
| 接收/分发用户消息 | Low | Chat 对话 |
| 架构设计 / 拆解需求 / 产出草案 | Medium | Planner、Assistant 分析 |
| 用户对话 / 状态汇报 / 消息提交 | Low | Chat 交互 |
| 消息分类 / 简单格式化 / 状态总结 | Low | 辅助任务 |

Executor 在唤起 Sub-Agent 时指定期望的 cost_tier：

```rust
// Executor → Sub-Agent gRPC 请求
message ExecuteTaskRequest {
    string task_id = 1;
    string task_type = 2;          // "code_gen" | "planning" | "review" | "formatting"
    CostTier min_cost_tier = 3;    // 最低模型等级要求
    // ... task context
}
```

Sub-Agent 从自身角色配置中选择满足 `min_cost_tier` 的第一个 Healthy 模型执行。

### 模型监控

**v0.1 不实现 model_health collection，仅通过 AgentInstance.model_state 维护模型状态。v0.2 引入独立 model_health collection。**

v0.1 Scheduler 读取 `AgentInstance.model_state` 进行模型可用性判断，避免大量 Agent 同时撞到同一不健康的模型。

v0.2 引入独立 `model_health` collection，由 API Server 聚合各 Agent 的模型状态形成全局视图：

```jsonc
// v0.2 MongoDB 独立 collection: model_health
{
    "_id": ObjectId,
    "model": "claude-sonnet-4-20250514",
    "provider": "anthropic",
    "status": "healthy",           // healthy | degraded | unhealthy
    "error_rate_last_hour": 0.02,
    "avg_latency_ms": 1200,
    "updated_at": ISODate
}
```

## 状态流转

### Project 状态流转
```
Scheduler Watch 到 Pending Project
     │
     ▼
  Pending ──(分配 Executor)──▶ Assigned
     ▲  │                  │
     │  │                  ├── 用户取消（通知 Executor）──▶ Cancelled
     │  │                  │
     │  │                  ├── Scheduler 对账：status=assigned && assigned_at < now - assign_timeout_sec
     │  │                  │   → pending（清理分配信息并回退）
     │  │                  │
     │  │                  │ Executor 开始执行
     │  │                  ▼
     │  └── 用户取消 / 级联取消 ─────────────────────────▶ Cancelled
     │                  InProgress
     │                     │
     │                     ├── 所有子任务完成 ──▶ Completed
     │                     │
     │                     ├── 等待用户反馈 ──▶ Blocked ──(反馈写入 ChatMessage 并 Resume)──▶ InProgress
     │                     │                     │
     │                     │                     └── 用户取消（长时间无法解决）──▶ Cancelled
     │                     │
     │                     ├── 用户取消（cooperative cancel，释放 worktree/merge_lock）──▶ Cancelled
     │                     │
     │                     └── 执行失败 ──▶ Failed

    Executor 崩溃恢复：InProgress（清空 assigned_executor_id）──▶ Pending（由 Scheduler 重新分配）

    - Pending → Cancelled：用户取消或 Requirement 级联取消
    - Assigned → Cancelled：用户取消，需通知 Executor
    - InProgress → Cancelled：用户取消；Executor 采用 cooperative cancel，停止当前执行并释放 worktree / merge_lock
    - Blocked → Cancelled：用户取消，适用于长时间无法解决的阻塞
```

### AgentInstance 生命周期
```
Scheduler 创建 Agent
     │
     ▼
Initializing ──(初始化成功)──▶ Idle ──(领取任务)──▶ Busy
     │                         ▲                    │
     │                         │                    │ 任务完成
     │                         │                    ▼
     │                         │                  Idle
     │                         │
     │                         │ (心跳超时)
     │                         ▼
     └──(初始化失败)────────▶ Error ──(Scheduler 回收)──▶ 销毁
```

Initializing 退出条件：Agent 注册成功 → Idle；超时 30s（如连接 DB 失败或依赖初始化未完成）→ Error。

### Scheduler 调度决策流程
```
Scheduler Watch 循环:

  Project 变更事件（status=pending 且无 assigned_executor_id）:
    │
    ├── Executor Pool 未达 max → 创建新 Executor 实例，分配 Project
    │
    └── Executor Pool 已达 max → Project 保持 Pending，等待下次 Watch

  实例空闲 > scale_down_idle_secs 且 当前数 > min:
    └── 回收实例（Deregister + 通知退出）
```

## Agent 生命周期管控

Scheduler 管理 Executor Pool 和 Assistant Pool。Chat 和 Evolver 独立管控，不与 Scheduler 耦合；Assistant 受 Scheduler 管辖。

| Agent | 模式 | 生命周期 | 管控者 |
|---|---|---|---|
| **Executor** | Pool（动态伸缩） | Scheduler 按 pending Project 数创建/销毁 | Scheduler |
| **Assistant** | 受 Scheduler 管辖（Pool） | Scheduler 按需创建/回收 | Scheduler |
| **Chat** | 随连绑定（无 Pool） | 用户连接 → 创建/复用；断连 + 超时 → 释放 | 连接层 |
| **Scheduler** | 单例 | 启动即注册，常驻 | — |
| **Evolver** | 单例 | 启动即注册，常驻后台，定时循环 | 自身 |

```
Chat —── 随连绑定（无 Pool）───
  Workspace 有用户连接 → 创建 / 复用 Chat
  用户断连 + 超时       → 释放资源

Assistant —── Pool（Scheduler Watch 驱动）───
  Requirement pending/analyzing → Scheduler 创建/调度 Assistant → 分析/草案
  Requirement 分析完成 / 空闲超时 → Scheduler 销毁

Evolver —── 单例 + 定时器 ───
  启动时注册为单例
  循环: sleep(interval) → 扫描已完成 Project → 提炼模式 → 产出 Skills/MCP → sleep(interval)

Executor —── Pool（Scheduler Watch 驱动）───
  Project pending → Scheduler 创建 Executor → 绑定分配
  Project 完成 / 空闲超时 → Scheduler 销毁
```

### 管控边界
- **Scheduler** 是唯一能做 Agent 创建/销毁动态决策的组件，管辖 Executor Pool 和 Assistant Pool
- Chat 生命周期由连接层（gRPC session）管理，Scheduler 不参与
- **Assistant** 作为后台 worker 由 Scheduler 调度，处理 Requirement 分析、Project/Task 拆解和结果汇总
- **Evolver** 自身维护定时循环，不受 Scheduler 调度

### 模型健康状态流转
```
Healthy ──(请求失败)──▶ Degraded ──(再次失败)──▶ Degraded ──(第3次失败)──▶ Unhealthy
   ▲                       │                                                  │
   │                       │ (成功)                                           │ (冷却 60s 后重试成功)
   │                       ▼                                                  ▼
   └────────────────── Healthy ◀──────────────────────────────(成功)──── Degraded

降级路径（按 RoleModelConfig 列表优先级）:
  同 tier 逐项降级 → 下一 tier（high → medium → low）→ 全部 Unhealthy → Scheduler 告警
```

## Scheduler 设计

### 职责
- v0.1 已定稿：Scheduler 单例；多 Scheduler/主备不作为开放问题，放到 v0.2 再评估
- 管理 Executor Pool + Assistant Pool（Chat 和 Evolver 不受 Scheduler 管辖）
- 根据 pending Project 数量动态创建/销毁 Executor 实例
- **将 Project 分派给 idle 状态的 Executor**（Project 粒度分配，非 Task 粒度）
- 分配前检查目标 Project 是否已有 assigned_executor_id，防止两个 Executor 分配到同一 Project
- Scheduler 对账负责 assigned 超时检测和回退：`status=assigned && assigned_at < now - assign_timeout_sec → pending`

### Project 独占分配机制
```
分配前提:
  - Project._id 天然保证单文档原子更新（_id 唯一）
  - AgentInstance.current_project_id 部分唯一索引用于防止多个 Executor 同时声明同一个 Project
  - 单个 Executor 同时只能处理一个 Project，由 AgentInstance 单文档字段和状态机保证

分配流程（必须在 MongoDB transaction 中执行）:
  1. 条件更新 Project:
     db.projects.updateOne(
       { _id: project_id, assigned_executor_id: { $exists: false }, status: "pending" },
       { $set: { assigned_executor_id: executor_id, status: "assigned" } }
     )
     若 matched_count=0 → Project 已被其他 Executor 抢占或状态不再 pending，事务回滚

  2. 条件更新 Executor:
     db.agent_instances.updateOne(
       { _id: executor_id, current_project_id: { $exists: false }, status: "idle" },
       { $set: { current_project_id: project_id, status: "busy" } }
     )
     若 matched_count=0 → Executor 忙或不存在，事务回滚

  3. Executor 收到分配并接受后，通过 ProjectService.Accept 将 Project status 从 assigned 改为 in_progress。

MongoDB 约束:
  - AgentInstance 部分唯一索引:
    db.agent_instances.createIndex(
      { current_project_id: 1 },
      { unique: true, partialFilterExpression: { current_project_id: { $exists: true } } }
    )
  - 不需要在 Project.assigned_executor_id 上建唯一索引（_id + 条件更新 + transaction 已保证 Project 分配互斥）
```

### 扩缩策略

Scheduler 管理 Executor Pool 和 Assistant Pool。Chat 随用户连接绑定；Assistant 由 Scheduler 调度（Pool），Evolver 为单例：

```
Executor Pool 大小 = f(pending_project_count, max_concurrent_executor)
```

每个 Pool 有 min/max 实例限制，按需在区间内自动伸缩。

### 调度流程
```
Scheduler Watch 循环:
  1. 收到 Project 变更事件（status=pending 且无 assigned_executor_id）
  2. 查询空闲 Executor
  3. 无空闲 Executor 且未达 max → 创建新 Executor
  4. 在 MongoDB transaction 中条件绑定 Project → Executor
  5. 更新 Project status → assigned
  6. 更新 Executor status → busy
  7. Executor 接受分配后调用 ProjectService.Accept，Project status → in_progress
```

## Evolver 设计（含 RAG）

Evolver 是对系统元认知的出口——它观察已完成的工作，提炼可复用模式，产出新的 Skills 和 MCP 配置，驱动系统自我进化。

### Embedding 写入时机

API Server 在写入以下文档时异步生成 embedding，并将向量与检索 payload 写入 Qdrant。MongoDB 只保存业务文档和 `embedding_ref`（Qdrant point id / collection 名），不承担向量检索职责。

| 文档 | embedding 内容 | 触发时机 |
|---|---|---|
| Requirement | 需求文本提炼（LLM 将长文本压缩为 512 token 摘要后向量化） | Requirement 写入时 |
| Project（状态=completed） | project name + 生成的 summary + 关键决策描述 | Project 完成时 |
| ProjectTask（状态=completed） | task name + Executor 产出摘要 + 遇到的坑/解法 | Task 完成时 |

### 反思流程
```
Evolver 定时循环（interval 可配，默认 24h）:

1. 扫描近期（~7d）已完成但未反思的 Project
2. 对每个 Project 做 embedding 检索:
   - 用"有哪些之前做过的类似项目？"查询 Project embedding
   - 用"有哪些反复出现的问题？"查询 ProjectTask embedding
   - 用"用户最近关注什么方向？"查询 Requirement embedding
3. LLM 综合检索到的上下文，产出:
   a. 模式总结（如"XXX 类需求推荐用 YYY 技术方案"）
   b. Skill 生成/优化（如自动生成 react-form 模板 Skill）
   c. MCP 建议（如检测到重复 API 调用 → 建议配置对应 MCP server）
4. 反思结果写入白板（reflections collection）
5. 标记 Project 为已反思
```

### Qdrant RAG 存储边界

RAG 不属于 P0 的硬依赖。向量数据统一存入 Qdrant，MongoDB 仅保存业务数据和向量引用：
- Qdrant collections：`requirements`、`projects`、`project_tasks`、`reflections`（同 shape，但不同 collection 可配置不同向量维度和 payload index；`reflections` 对应 reflections schema 中 `embedding_ref.collection = "reflections"`）。
- Point id：使用对应 MongoDB 文档 `_id` 的字符串形式，便于回查业务文档。
- Payload：保存 `workspace_id`、`document_type`、`source_id`、`status`、`created_at`、`updated_at` 等过滤字段。
- 本地开发：通过 Docker 启动 Qdrant；未配置 Qdrant 时禁用 RAG，Evolver 跳过相似检索步骤，只做规则化总结。
- 一致性：MongoDB 写入成功后异步写 Qdrant；Qdrant 写失败不回滚业务写入，记录 `embedding_status=failed` 并由后台 worker 重试。

### Qdrant Collection 配置
```jsonc
{
  "collection_name": "project_tasks",
  "vectors": {
    "size": 1536,
    "distance": "Cosine"
  },
  "payload_indexes": [
    "workspace_id",
    "document_type",
    "status",
    "updated_at"
  ]
}
```

// same shape for requirements, projects, reflections

## Agent 实现：模板 + 装配器

### 通用模板（`agents/src/template.rs`）
Agent 运行时核心：
- LLM 对话循环（当前 agent_runner 提取/抽象）
- 工具调用执行
- 上下文管理（压缩、token 估算）
- 结果汇总

### 装配器（`agents/src/assembler.rs`）
根据角色配置组装 Agent：
```
assembler.assemble(role: RoleConfig) -> ConfiguredAgent {
    system_prompt: role.system_prompt,
    skills: role.skills,
    mcp_servers: role.mcp.servers,
    can_call_roles: role.permissions.can_call_roles,
    model_selector: ModelSelector::from(role.models),
    tools: role.permissions.allowed_tools,
}
```

### 角色配置（`agents/roles/`）

```toml
# chat.toml（面向用户的对话 Agent）
[agent]
name = "chat"
role = "chat"
pool_size = 0               # 随连绑定，无 Pool

[model]
provider = "anthropic"
model = "claude-sonnet-4-20250514"

[tools]
allowed_tools = ["read", "write", "web_search", "web_fetch"]

# assistant.toml（后台需求分析/草案 Worker）
[agent]
name = "assistant"
role = "assistant"
pool_size = 3               # Scheduler 管理 Pool

[model]
provider = "deepseek"
model = "deepseek/deepseek-chat"

[tools]
allowed_tools = ["read", "write", "grep", "glob"]
```

```toml
# evolver.toml
[role]
name = "evolver"
description = "定期扫描白板，提炼模式，生成/优化 Skills 和 MCP"

[[models]]
model = "deepseek/deepseek-chat"
cost_tier = "low"

[permissions]
allowed_tools = ["WebSearch"]
scope = ["board_read", "board_write"]
max_subagents = 0
can_call_roles = []
can_create_agents = false

[skills]
enabled = ["analysis", "summarization"]

[mcp]
servers = []
```

```toml
# scheduler.toml
[role]
name = "scheduler"
description = "管理 Agent Pool 生命周期，分派任务"
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

```toml
# executor.toml
[role]
name = "executor"
description = "领取 Project，编排 Sub-Agent 执行 Tasks，写回白板"

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

## 项目结构变更

### Crate 依赖关系
```
ui/              # 纯 UI ──HTTP/WS──▶ server
                 #   ──依赖──▶ share/openapi/sdk/ts
server/          # API Server ──依赖──▶ share
agents/          # Agent 运行时（独立部署）──依赖──▶ share
                 #   ──gRPC──▶ server
cli/             # CLI（保留）──依赖──▶ share
share/           # 共享层
  ├── core       #   共享核心库（类型、错误、工具抽象）
  ├── llm        #   LLM 客户端
  ├── tools      #   工具注册
  ├── proto/     #   gRPC protobuf 定义
  │   └── sdk/   #   生成的 gRPC SDK（rust + ts）
  └── openapi/   #   OpenAPI 3 schema（REST + WS）
      └── sdk/   #   生成的 API SDK（rust + ts）
```

### 目录结构

```
aemeath/
├── share/                        # ★ 共享层
│   ├── aemeath-core/             #   核心库（不变）
│   ├── aemeath-llm/              #   LLM 客户端（不变）
│   ├── aemeath-tools/            #   工具注册（不变）
│   ├── proto/                    #   gRPC protobuf 定义
│   │   ├── chat.proto
│   │   ├── workspace.proto
│   │   ├── requirement.proto
│   │   ├── project.proto
│   │   ├── project_task.proto
│   │   ├── agent.proto
│   │   ├── common.proto           #   共享枚举/类型（如 CostTier）
│   │   └── sdk/                  #   proto 生成的 SDK
│   │       ├── rust/             #     tonic 生成
│   │       └── ts/               #     protobuf-ts 生成
│   └── openapi/                  #   OpenAPI 3 schema
│       ├── spec.yaml             #     REST + WS 接口定义
│       └── sdk/                  #     OpenAPI 生成的 SDK
│           ├── rust/             #       Rust SDK（reqwest）
│           └── ts/               #       TypeScript SDK（fetch）
├── cli/                          # CLI（保留）
│   └── src/main.rs
├── server/                       # ★ API Server（按 feature 组织）
│   ├── Cargo.toml
│   ├── src/
│   │   ├── main.rs               #   服务入口（组装 feature）
│   │   ├── share/                #   server 内部共享层（feature 间通信接口）
│   │   │   ├── mod.rs
│   │   │   ├── types.rs          #     共享类型（WorkspaceId, ProjectId 等）
│   │   │   ├── repo_traits.rs    #     各 feature repository 暴露的 trait
│   │   │   └── event_bus.rs      #     内部事件总线（feature 间解耦通知）
│   │   └── features/             #   feature 模块
│   │       ├── chat/             #     chat feature
│   │       │   ├── mod.rs        #       对外暴露 pub 模块声明
│   │       │   ├── grpc.rs
│   │       │   ├── rest.rs
│   │       │   └── repository.rs
│   │       ├── workspace/        #     workspace feature
│   │       │   ├── mod.rs
│   │       │   ├── grpc.rs
│   │       │   ├── rest.rs
│   │       │   └── repository.rs
│   │       ├── requirement/      #     requirement feature
│   │       │   ├── mod.rs
│   │       │   ├── grpc.rs
│   │       │   ├── rest.rs
│   │       │   └── repository.rs
│   │       ├── project/          #     project feature
│   │       │   ├── mod.rs
│   │       │   ├── grpc.rs
│   │       │   ├── rest.rs
│   │       │   └── repository.rs
│   │       ├── project_task/     #     project_task feature
│   │       │   ├── mod.rs
│   │       │   ├── grpc.rs
│   │       │   ├── rest.rs
│   │       │   └── repository.rs
│   │       ├── agent/            #     agent feature
│   │       │   ├── mod.rs
│   │       │   ├── grpc.rs
│   │       │   ├── rest.rs
│   │       │   └── repository.rs
│   │       ├── board/            #     board feature（白板聚合）
│   │       │   ├── mod.rs
│   │       │   ├── rest.rs       #       GET /board/{workspace_id}
│   │       │   └── aggregator.rs #       跨 feature 聚合逻辑
│   │       └── ws/               #     WebSocket feature
│   │           ├── mod.rs
│   │           └── handler.rs    #       WS 连接管理 + BoardSnapshot 推送
├── agents/                       # ★ Agent 运行时（独立部署，按 role 组织）
│   ├── Cargo.toml
│   ├── src/
│   │   ├── main.rs               #   Agent 进程入口
│   │   ├── share/                #   agents 内部共享层（role 间通信接口）
│   │   │   ├── mod.rs
│   │   │   ├── types.rs          #     共享类型（AgentId / ProjectId / TaskContext 等）
│   │   │   ├── template.rs       #     通用 Agent 模板 trait
│   │   │   └── pool.rs           #     Pool trait（Executor 实现）
│   │   └── features/             #   role feature 模块
│   │       ├── scheduler/        #     Scheduler（调度 + Watch + 对账）
│   │       │   ├── mod.rs
│   │       │   └── scheduler.rs
│   │       ├── executor/         #     Executor（任务执行 + Pool 管理）
│   │       │   ├── mod.rs
│   │       │   └── executor.rs
│   │       ├── evolver/          #     Evolver（反思引擎 + 知识优化）
│   │       │   ├── mod.rs
│   │       │   └── evolver.rs
│   │       ├── chat/             #     Chat（用户对话 + 汇报）
│   │       │   ├── mod.rs
│   │       │   └── chat.rs
│   │       ├── assistant/        #     Assistant（后台需求分析/草案 Worker，由 Scheduler 调度）
│   │       │   ├── mod.rs
│   │       │   └── assistant.rs
│   │       └── sub_agent/        #     Sub-Agent（进程内 tokio task 执行）
│   │           ├── mod.rs
│   │           └── sub_agent.rs
│   └── roles/                    #   角色配置（TOML）
│       ├── chat.toml
│       ├── assistant.toml
│       ├── scheduler.toml
│       ├── executor.toml
│       ├── evolver.toml
│       ├── planner.toml
│       ├── coder.toml
│       ├── tester.toml
│       ├── reviewer.toml
│       ├── designer.toml
│       └── custom/
├── ui/                           # ★ 纯 Web 前端（Vue 3 + Element Plus，Vite 构建）
│   ├── package.json
│   ├── tsconfig.json
│   ├── vite.config.ts
│   └── src/
│       ├── views/                # 页面：会话 / 白板 / 需求详情 / 项目管理
│       ├── components/           # 组件：ChatMessage / BoardCard / StatusBadge / DAGView
│       ├── composables/          # useBoardSnapshot / useWS / useChat
│       ├── stores/               # Pinia 状态管理
│       └── lib/                  # 工具函数、类型定义
├── CLAUDE.md
├── TODO.md
└── docs/
```

## REST / WebSocket API 设计（前端接口）

Server 通过 REST + WebSocket 为前端白板提供数据。

### REST 端点
```
GET    /api/workspaces/:ws_id/board              # 白板聚合数据（一次性返回全部区块）
GET    /api/workspaces/:ws_id/chats/:chat_id/messages  # Chat 消息列表（含需求消息）
GET    /api/workspaces/:ws_id/requirements       # Requirement 列表（支持 ?status=... 过滤）
GET    /api/workspaces/:ws_id/requirements/:id   # Requirement 详情
POST   /api/workspaces/:ws_id/requirements       # 创建 Requirement
PATCH  /api/workspaces/:ws_id/requirements/:id   # 更新 Requirement（草案/状态/关联）
DELETE /api/workspaces/:ws_id/requirements/:id   # 删除/取消 Requirement
GET    /api/workspaces/:ws_id/projects           # Project 列表（支持 ?status=in_progress&requirement_id=... 过滤）
GET    /api/workspaces/:ws_id/projects/:id/tasks # 某个 Project 的 Task 列表
POST   /api/workspaces/:ws_id/projects/:id/resume # 用户反馈已写入 ChatMessage(message_type=feedback) 并关联 Project/Task 后，恢复 Blocked Project
GET    /api/workspaces/:ws_id/projects/:id/tasks/:task_id  # Task 详情
PATCH  /api/workspaces/:ws_id/projects/:id/tasks/:task_id  # 更新 Task 状态；status=cancelled 表示取消 Task
GET    /api/workspaces/:ws_id/agents             # Agent 实例列表
POST   /api/workspaces/:ws_id/chats/:chat_id/messages  # 创建 ChatMessage（Chat/用户调用）
POST   /api/workspaces/:ws_id/requirements/:id/confirm  # 确认草案并创建 Project/Task
POST   /api/workspaces/:ws_id/requirements/:id/reject   # 驳回草案
```

### WebSocket
```
WS /ws/workspaces/:ws_id/board
  → 实时推送白板数据变更事件：
    - chat_message_updated
    - requirement_updated
    - project_created
    - task_status_changed
    - agent_status_changed
```

### Board 聚合响应结构
```rust
#[derive(Serialize)]
// Workspace 文档的核心子集（workspace_id / name / created_at / updated_at）
pub struct BoardSnapshot {
    pub workspace: WorkspaceInfo,
    pub chats: Vec<Chat>,                         // Chat 会话
    pub recent_messages: Vec<ChatMessage>,        // 近期 Chat 消息（默认最近 50 条）
    pub requirements: Vec<Requirement>,           // Requirement 记录与草案
    pub projects: Vec<ProjectWithTasks>,           // Project & Tasks
    pub agent_instances: Vec<AgentInstance>,       // Agent 状态
}

#[derive(Serialize)]
pub struct ProjectWithTasks {
    pub project: Project,
    pub tasks: Vec<ProjectTask>,
}
```

## gRPC API 设计（Agent 间通信）

### Common Types（common.proto）

多个 Service 共享的 Watch 请求定义放在 `common.proto`：

```protobuf
message WatchRequest {
  string workspace_id = 1;
  string resume_token = 2;  // 断线恢复用，首次为空
}
```

### Chat Service
```protobuf
service ChatService {
    rpc Create(CreateChatRequest) returns (Chat);
    rpc AddMessage(AddMessageRequest) returns (ChatMessage);
    rpc AnalyzeMessage(AnalyzeMessageRequest) returns (AnalyzeMessageResponse);  // Chat 分析消息类型（requirement / feedback / chitchat）
    rpc Get(GetChatRequest) returns (Chat);
    rpc List(ListChatsRequest) returns (ListChatsResponse);
    rpc Watch(WatchRequest) returns (stream ChatEvent);
  }

message Chat {
  string chat_id = 1;
  string workspace_id = 2;
  string title = 3;
  string status = 4;
  uint64 version = 5;
}

message ChatMessage {
  string message_id = 1;
  string chat_id = 2;
  string workspace_id = 3;
  string role = 4;
  string content = 5;
  string message_type = 6;
  string requirement_id = 7;
  uint64 version = 8;
}

message CreateChatRequest {
  string workspace_id = 1;
  string title = 2;
}

message AddMessageRequest {
  string workspace_id = 1;
  string chat_id = 2;
  string role = 3;
  string content = 4;
}

message AnalyzeMessageRequest {
    string workspace_id = 1;
    string chat_id = 2;
    string message_id = 3;
  }

  message AnalyzeMessageResponse {
    string message_type = 1;  // requirement / feedback / chitchat
    string summary = 2;       // 消息摘要
  }

  message GetChatRequest {
  string workspace_id = 1;
  string chat_id = 2;
}

message ListChatsRequest {
  string workspace_id = 1;
  int32 page_size = 2;
  string page_token = 3;
}

message ListChatsResponse {
  repeated Chat chats = 1;
  string next_page_token = 2;
}

message ChatEvent {
  string event_type = 1;
  Chat chat = 2;
  ChatMessage message = 3;
  string resume_token = 4;
}
```

### Workspace Service
```protobuf
service WorkspaceService {
  rpc Create(CreateWorkspaceRequest) returns (Workspace);
  rpc Get(GetWorkspaceRequest) returns (Workspace);
  rpc List(ListWorkspacesRequest) returns (ListWorkspacesResponse);
  rpc Watch(WatchRequest) returns (stream WorkspaceEvent);
}

message Workspace {
  string workspace_id = 1;
  string tenant_id = 2;
  string name = 3;
}

message CreateWorkspaceRequest {
  string tenant_id = 1;
  string name = 2;
}

message GetWorkspaceRequest {
  string workspace_id = 1;
}

message ListWorkspacesRequest {
  string tenant_id = 1;
  int32 page_size = 2;
  string page_token = 3;
}

message ListWorkspacesResponse {
  repeated Workspace workspaces = 1;
  string next_page_token = 2;
}

message WorkspaceEvent {
  string event_type = 1;
  Workspace workspace = 2;
  string resume_token = 3;
}
```

### Requirement Service
```protobuf
service RequirementService {
  rpc Create(CreateRequirementRequest) returns (Requirement);
  rpc Update(UpdateRequirementRequest) returns (Requirement);
  rpc Get(GetRequirementRequest) returns (Requirement);
  rpc List(ListRequirementsRequest) returns (ListRequirementsResponse);
  rpc Analyze(AnalyzeRequirementRequest) returns (Requirement);   // Assistant 原子抢占（后台 worker） pending → analyzing
  rpc Confirm(ConfirmRequirementRequest) returns (Requirement);   // 用户确认草案，生成 Project/Task
  rpc Reject(RejectRequirementRequest) returns (Requirement);     // 用户驳回草案
  rpc Watch(WatchRequest) returns (stream RequirementEvent);
}

message Requirement {
  string requirement_id = 1;
  string workspace_id = 2;
  string source_message_id = 3;
  string title = 4;
  string description = 5;
  string category = 6;
  string status = 7;
  repeated string project_ids = 8;
  repeated string task_ids = 9;
  string draft_json = 10;
  uint64 version = 11;
}

message CreateRequirementRequest {
  string workspace_id = 1;
  string source_message_id = 2;
  string title = 3;
  string description = 4;
}

message UpdateRequirementRequest {
  string workspace_id = 1;
  string requirement_id = 2;
  string title = 3;
  string description = 4;
  string draft_json = 5;
  uint64 expected_version = 6;
}

message GetRequirementRequest {
  string workspace_id = 1;
  string requirement_id = 2;
}

message ListRequirementsRequest {
  string workspace_id = 1;
  string status = 2;
  int32 page_size = 3;
  string page_token = 4;
}

message ListRequirementsResponse {
  repeated Requirement requirements = 1;
  string next_page_token = 2;
}

message AnalyzeRequirementRequest {
  string workspace_id = 1;
  string requirement_id = 2;
  string assistant_agent_id = 3;
  uint64 expected_version = 4;
}

message ConfirmRequirementRequest {
  string workspace_id = 1;
  string requirement_id = 2;
  string feedback_message_id = 3;
  uint64 expected_version = 4;
}

message RejectRequirementRequest {
  string workspace_id = 1;
  string requirement_id = 2;
  string feedback_message_id = 3;
  string reason = 4;
  uint64 expected_version = 5;
}

message RequirementEvent {
  string event_type = 1;
  Requirement requirement = 2;
  string resume_token = 3;
}
```

### Project Service
```protobuf
service ProjectService {
  rpc Create(CreateProjectRequest) returns (Project);
  rpc Update(UpdateProjectRequest) returns (Project);
  rpc Get(GetProjectRequest) returns (Project);
  rpc List(ListProjectsRequest) returns (ListProjectsResponse);
  rpc Assign(AssignProjectRequest) returns (Project);      // Scheduler 在事务中分配 Project → Executor
  rpc Accept(AcceptProjectRequest) returns (Project);      // Executor 接受分配，assigned → in_progress
  rpc Resume(ResumeProjectRequest) returns (ResumeProjectResponse); // 用户反馈已写入后，blocked → in_progress
  rpc Complete(CompleteProjectRequest) returns (Project);  // Executor 标记 Project 完成
  rpc Watch(WatchRequest) returns (stream ProjectEvent);
}

message Project {
  string project_id = 1;
  string workspace_id = 2;
  repeated string requirement_ids = 3;
  string assigned_executor_id = 4;
  string name = 5;
  string status = 6;
  string summary = 7;
  repeated string key_decisions = 8;
  uint64 version = 9;
}

message CreateProjectRequest {
  string workspace_id = 1;
  repeated string requirement_ids = 2;
  string name = 3;
}

message UpdateProjectRequest {
  string workspace_id = 1;
  string project_id = 2;
  string name = 3;
  string status = 4;
  string summary = 5;
  uint64 expected_version = 6;
}

message GetProjectRequest {
  string workspace_id = 1;
  string project_id = 2;
}

message ListProjectsRequest {
  string workspace_id = 1;
  string status = 2;
  int32 page_size = 3;
  string page_token = 4;
}

message ListProjectsResponse {
  repeated Project projects = 1;
  string next_page_token = 2;
}

message AssignProjectRequest {
  string workspace_id = 1;
  string project_id = 2;
  string executor_id = 3;
  uint64 expected_version = 4;
}

message AcceptProjectRequest {
  string workspace_id = 1;
  string project_id = 2;
  string executor_id = 3;
  uint64 expected_version = 4;
}

message ResumeProjectRequest {
  string workspace_id = 1;
  string project_id = 2;
  string feedback_message_id = 3; // ChatMessage(message_type=feedback)，metadata 关联 Project/Task
  string task_id = 4;             // 可选：反馈针对的具体 Task
  uint64 expected_version = 5;    // Project.version 乐观锁
}

message ResumeProjectResponse {
  Project project = 1;
}

message CompleteProjectRequest {
  string workspace_id = 1;
  string project_id = 2;
  string executor_id = 3;
  string summary = 4;
  repeated string key_decisions = 5;
  uint64 expected_version = 6;
}

message ProjectEvent {
  string event_type = 1;
  Project project = 2;
  string resume_token = 3;
}
```

### Project Task Service
```protobuf
service ProjectTaskService {
  rpc Create(CreateTaskRequest) returns (ProjectTask);
  rpc Update(UpdateTaskRequest) returns (ProjectTask);
  rpc Assign(AssignTaskRequest) returns (ProjectTask);          // Executor 内部编排 Task 时绑定执行者
  rpc Complete(CompleteTaskRequest) returns (ProjectTask);      // Executor 标记完成
  rpc List(ListTasksRequest) returns (ListTasksResponse);
  rpc Watch(WatchRequest) returns (stream ProjectTaskEvent);
  rpc CancelTask(CancelTaskRequest) returns (CancelTaskResponse); // 完整 Request/Response 定义见 [§9.1](#91-长任务超时与取消)
}

message ProjectTask {
  string task_id = 1;
  string project_id = 2;
  string workspace_id = 3;
  string title = 4;
  string description = 5;
  string status = 6;
  string assigned_executor_id = 7;
  string executor_type = 8;
  repeated string depends_on = 9;
  string depends_type = 10;
  int32 priority = 11;
  uint64 version = 12;
}

message CreateTaskRequest {
  string workspace_id = 1;
  string project_id = 2;
  string title = 3;
  string description = 4;
  string executor_type = 5;
  repeated string depends_on = 6;
  string depends_type = 7;
  int32 priority = 8;
}

message UpdateTaskRequest {
  string workspace_id = 1;
  string task_id = 2;
  string title = 3;
  string description = 4;
  string status = 5;
  uint64 expected_version = 6;
}

message AssignTaskRequest {
  string workspace_id = 1;
  string task_id = 2;
  string executor_instance_id = 3;
  uint64 expected_version = 4;
}

message CompleteTaskRequest {
  string workspace_id = 1;
  string task_id = 2;
  string executor_instance_id = 3;
  string output_summary = 4;
  uint64 expected_version = 5;
}

message ListTasksRequest {
  string workspace_id = 1;
  string project_id = 2;
  string status = 3;
  int32 page_size = 4;
  string page_token = 5;
}

message ListTasksResponse {
  repeated ProjectTask tasks = 1;
  string next_page_token = 2;
}

message CancelTaskRequest {
  string workspace_id = 1;
  string task_id = 2;
  string reason = 3;
  uint64 expected_version = 4;
}

message CancelTaskResponse {
  ProjectTask task = 1;
}

message ProjectTaskEvent {
  string event_type = 1;
  ProjectTask task = 2;
  string resume_token = 3;
}
```

### Agent Registry Service
```protobuf
service AgentRegistryService {
  rpc Register(RegisterAgentRequest) returns (AgentInstance);
  rpc Heartbeat(HeartbeatRequest) returns (HeartbeatResponse);
  rpc Deregister(DeregisterAgentRequest) returns (Empty);
  rpc List(ListAgentsRequest) returns (ListAgentsResponse);
  rpc Watch(WatchRequest) returns (stream AgentEvent);
  rpc RefreshToken(RefreshTokenRequest) returns (RefreshTokenResponse);
}

message RegisterAgentRequest {
  string agent_id = 1;
  AgentType agent_type = 2;
  string executor_id = 3;
  string workspace_id = 4;
}

message AgentInstance {
  string agent_id = 1;
  string workspace_id = 2;
  AgentType agent_type = 3;
  string role = 4;
  string status = 5;
  string active_model = 6;
  string current_project_id = 7;
  uint64 version = 8;
}

message HeartbeatRequest {
  string agent_id = 1;
  string workspace_id = 2;
  string status = 3;
  string current_project_id = 4;
}

message HeartbeatResponse {
  bool ok = 1;
  string access_token = 2;
}

message DeregisterAgentRequest {
  string agent_id = 1;
  string workspace_id = 2;
  string reason = 3;
}

message Empty {
}

message ListAgentsRequest {
  string workspace_id = 1;
  AgentType agent_type = 2;
  string status = 3;
}

message ListAgentsResponse {
  repeated AgentInstance agents = 1;
}

message AgentEvent {
  string event_type = 1;
  AgentInstance agent = 2;
  string resume_token = 3;
}

message RefreshTokenRequest {
  string agent_id = 1;
  string refresh_token = 2;
}

message RefreshTokenResponse {
  string access_token = 1;
  string refresh_token = 2;
  int64 expires_in = 3;
}
```

### Board Service
```protobuf
service BoardService {
  rpc Watch(WatchRequest) returns (stream BoardSnapshot);
  rpc GetBoard(GetBoardRequest) returns (BoardSnapshot);
  rpc GetBoardSnapshot(GetBoardRequest) returns (BoardSnapshot);
}

message GetBoardRequest {
  string workspace_id = 1;
}
```

## 实现前必须定稿的架构决策

以下关键设计项如果在 spec 阶段不定稿，实现时必定返工。每项给出当前决策、影响范围、未决问题和默认方案。

---

### 1. 状态机与合法迁移

#### 当前决策

```
  Requirement（关联方式：1 Requirement N:N Project，完成判定看 ProjectTask）:
    Pending ──▶ Analyzing ──▶ Draft ──▶ InProgress ──▶ Completed
                   ▲              │         │
                   │              ▼         ▼
                   └────────── Rejected ◀── (用户驳回)

                   Analyzing ──(超时 120s 无产出)──▶ Pending
    - 多方并发控制：Analyzing 标记由 Assistant 原子抢占
    - Draft 允许多轮修改（Draft → Draft），draft_history 追加入口（revision++）
    - Draft 内有 projects/tasks 草案
    - ConfirmRequirement 事务中一次性完成（v0.1，无异步）：
      1. 创建 Project（含 Task 草案）
      2. 创建 ProjectTask
      3. 回填 `Requirement.project_ids`（append 新 Project ID）
      4. 回填 `Requirement.task_ids`（append 新 Task ID）
      5. `Requirement.status → InProgress`
      6. `Requirement.version++`
    - InProgress→Completed：所有 task_ids 中的 ProjectTask 为 Completed 或 Cancelled
    - Rejected → Analyzing：用户重新提交后重启分析流程
    - 任意状态 → Cancelled：用户主动取消（级联取消关联 Project 和 Task）
    - **不使用 Confirmed 状态**（v0.1 简化：Confirm RPC 同步完成所有操作，直接进 InProgress）

Project:
    Pending ──▶ Assigned ──▶ InProgress ──▶ Completed
      │          │             │  │
      │          │             │  ├──▶ Failed
      │          │             │  │
      │          │             │  ├──▶ Blocked ──▶ InProgress (用户反馈)
      │          │             │  │       │
      │          │             │  │       └──▶ Cancelled
      │          │             │  │
      │          │             │  └──▶ Cancelled
      │          │             │
      │          │             └── Executor 崩溃恢复 ──▶ Pending
      │          │
      │          ├── (超时) ──▶ Pending
      │          └──▶ Cancelled
      │
      └──▶ Cancelled

    Executor 崩溃恢复：InProgress（清空 assigned_executor_id）──▶ Pending（重新分配）

    - Assigned 超时门限：assign_timeout_sec（60s），超时回退 Pending
    - Pending → Cancelled：用户取消或 Requirement 级联取消
    - Assigned → Cancelled：用户取消，需通知 Executor
    - InProgress → Cancelled：用户取消；Executor 采用 cooperative cancel，停止当前执行并释放 worktree / merge_lock
    - Project 进入 Blocked 后由 Agent 主动提醒用户；无系统自动超时，用户通过反馈解锁或手动取消
    - Blocked → InProgress：用户反馈通过 `ChatMessage(message_type=feedback)` 写入并关联 Project/Task，然后调用 ProjectService.Resume / REST resume 入口恢复执行
    - Blocked → Cancelled：用户主动取消，适用于长时间无法解决的阻塞
    - Failed 为普通执行失败终态之一，不自动回退 Pending；只有显式人工重试/重开才可创建新的分配流程
    - Executor 崩溃恢复时，Scheduler 可将 InProgress Project 清空 assigned_executor_id 并回退 Pending 以重新分配
    - Completed 后冻结

ProjectTask:
  Pending ──▶ InProgress ──▶ InReview ──▶ Completed
                ▲   │            │
                │   │            └──▶ InProgress (Review 不通过返工)
                │   │
                │   └── retry_needed / 可重试失败 ──▶ Retrying
                │                                      │
                └──────────────────────────────────────┘
                  （下一次 attempt 开始：Retrying→InProgress）

  Pending ──▶ Cancelled
  InProgress / InReview / Retrying ──(Executor 崩溃恢复)──▶ Pending
  InProgress / Retrying ──(超过 max_task_retries 或不可重试失败)──▶ Failed

  - InProgress→Retrying：Sub-Agent 返回 retry_needed 或 Executor 判定本次 attempt 可重试时；写 intermediate attempt result，不写 final result
  - Retrying→InProgress：下一次 attempt 开始执行时
  - InProgress/Retrying→Failed：超过 max_task_retries（默认 3）或不可重试失败时，写 final failed result；普通失败不自动回退 Pending
  - InReview→InProgress：Reviewer 返回修改意见时
  - InProgress/InReview/Retrying→Pending：仅 Executor 崩溃恢复时由 Scheduler 回退并清空 assigned_executor_id
  - Pending→Cancelled：用户取消 / Project 被取消时级联
  - InProgress/InReview/Retrying 均可直接通过 `CancelTask` → Cancelled；Executor 采用 cooperative cancel，收到取消信号后尽快停止当前 attempt、释放资源并写回最终取消状态

AgentInstance:
  Idle ──▶ Busy ──▶ Idle
    │         │
    ▼         ▼
  Error ──▶ Idle (恢复) / 销毁 (Scheduler 回收)

  - Error→Idle：暂时性错误（模型超时/熔断），冷却后恢复
  - Error→Deregister：Scheduler 心跳检测超时 → 销毁，释放 Project 绑定
  - 新增 Initializing：Scheduler 创建后到 Idle 之间的过渡
```

#### 影响范围
- `Requirement.draft` 字段（草案 JSON、draft_history、revision）
- Project / ProjectTask MongoDB schema（status 字段枚举值 + 条件更新的 filter）
- gRPC Request / Response 的字段（`status` + `version`）
- Scheduler 决策逻辑（超时检测、崩溃恢复时的重新分配条件）
- Executor 编排流程（Task 状态流转、失败重试）
- WebSocket 推送事件（状态变更通知前端渲染）

#### 已定稿决策
- `draft_history.revision` 必须存在；每次 Draft→Draft 追加历史时 `revision++`，`Requirement.version` 仅用于乐观锁并发控制并随更新 `$inc`。
- `InProgress` / `InReview` / `Retrying` 均可直接 `CancelTask → Cancelled`，由 Executor cooperative cancel 停止当前 attempt 并释放资源。

#### 默认方案
- 文档中所有枚举值的 status 字段使用上述状态机
- `version: u64` 字段每次更新 `$inc`
- 草案保留修改历史在 `Requirement.draft_history: [{ revision, draft, timestamp }]`，其中 `revision` 为必填字段并单调递增；`Requirement.version` 用于乐观锁并发控制

---

### 2. Executor 编排模型

#### 当前决策

Executor 接收 Project 后，按 DAG（有向无环图）编排 Task 执行：

```
执行模式: DAG
  - 按 ProjectTask.depends_on 拓扑排序
  - 无依赖 Task 进入就绪队列，受 max_concurrent_tasks 限制
  - Task 完成后检查下游依赖，满足条件的下游 Task 进入就绪队列

依赖声明（ProjectTask）:
  depends_on: [task_id]              // 前置 Task ID 列表
  depends_type: "all" | "any"        // all = 全部完成，any = 任一完成

Confirm 阶段对 depends_on 做拓扑校验，有环直接拒绝确认草案。

并发控制:
  max_concurrent_tasks: usize        // 最大同时执行数（可配，默认 5）
```

Sub-Agent 调用流程：
```
Executor 对每个就绪 Task:
  1. 查询 Task.executor_type（如 "coder" / "planner"）
  2. 通过本地角色配置目录（roles/）查可用的 Sub-Agent 角色列表
  3. 装配子 Agent 上下文（request context: task description + related_messages + project context）
  4. 进程内启动 tokio task，调用 SubAgent::run → Sub-Agent 执行
  5. 收集当前批次所有 Task 的结果
  6. 批次完成后统一写回白板（方便 Chat 一次性感知进度，避免碎片推送）
  7. 根据结果判定 Task 状态（completed / failed / retry_needed）
     - completed / failed：写 ProjectTaskResult 作为 final result，并更新 ProjectTask.status
     - retry_needed：不写 final result；Executor 写一条 ProjectTaskResult(status=retry_needed) 作为 intermediate attempt result（记录本次 attempt 的 output/error_message/artifacts/retry_count），将 ProjectTask.status 置为 retrying，然后按 retry 策略重新执行该 Task
  8. retry: 失败或 retry_needed 后最多 max_task_retries 次（可配，默认 3）；最终超过 max_task_retries 后写 ProjectTaskResult(status=failed) 作为最终结果，并将 ProjectTask.status 置为 failed
```

#### 影响范围
- `ProjectTask` schema（depends_on、depends_type、executor_type 字段）
- `agent.proto`（ExecuteTask RPC 定义）
- Executor 运行时（DAG 拓扑排序 + 批次收集）
- 前端渲染（Task DAG 可视化）

#### 当前决策
- Executor 内部 DAG 执行器使用 tokio JoinSet。
- `Task.result_type` 省略该字段。

#### 默认方案
- v0.1 直接实现 DAG 模式
- 所有依赖声明用 Task ID，不支持外部资源依赖
- 不引入 result_type，Sub-Agent 输出直接透传给下游 Task

---

### 3. Sub-Agent 调用协议

#### 部署模型（v0.1 定稿）

**Sub-Agent 是 Executor 进程内的 tokio task**，不是独立进程。

```
Executor 进程
  ├── main.rs              # Executor 主循环
  ├── assembler.rs         # 装配器
  └── sub_agent.rs         # Sub-Agent 执行（进程内调用）

调用链路:
  Executor.execute_task()
    → SubAgent::run(config, task_description, project_context)
    → 返回 TaskResult
```

核心决策：
- Sub-Agent **不注册、不心跳、不直接连 API Server**
- Executor 本地装配 Sub-Agent 上下文，直接调用 `SubAgent::run()`
- Executor 通过 gRPC 向 Server 写 TaskResult（事实层）

#### ExecuteTask（进程内函数签名）

```rust
// agents/src/sub_agent.rs
pub struct ExecuteTaskParams {
    pub task_id: String,
    pub project_id: String,
    pub role: String,                   // "coder" / "planner" / ...
    pub task_description: String,
    pub project_context: String,        // LLM 摘要的 Project 上下文
    pub retry_count: u32,
    pub last_error: Option<String>,
}

pub enum SubAgentTaskStatus {
    Completed,
    Failed,
    RetryNeeded,
}

pub struct ExecuteTaskResult {
    pub task_id: String,
    pub status: SubAgentTaskStatus,    // completed | failed | retry_needed；Sub-Agent 执行结果枚举，独立于 DB 层 ProjectTaskStatus
    pub output: String,                // Sub-Agent 产出；retry_needed 时为本次尝试的中间输出/诊断
    pub artifacts: Vec<String>,        // 产出物引用（file_path / snippet_id）
}
```

`ExecuteTaskResult.status` 使用独立枚举 `SubAgentTaskStatus = completed | failed | retry_needed`，与 DB 层 `ProjectTaskStatus` 分离。`retry_needed` 由 Executor 映射为 `ProjectTaskStatus::Retrying`。Sub-Agent 返回 `ExecuteTaskResult.status = retry_needed` 表示“本次 attempt 未形成最终结果，但建议 Executor 重试”。Executor 必须写入一条 `ProjectTaskResult(status=retry_needed)` 作为 intermediate attempt result，保留本次 attempt 的 `output` / `artifacts` / `error_message` / `retry_count`，同时将 `ProjectTask.status` 更新为 `retrying`；该记录不作为 final result，不触发下游依赖完成判定。随后 Executor 使用递增后的 `retry_count` 与 `last_error` 重新调用 Sub-Agent。若最终超过 `max_task_retries` 仍未成功，Executor 写入最终 `ProjectTaskResult(status=failed)`，并将 `ProjectTask.status` 置为 `failed`。

Sub-Agent 不持有 Session，每次 ExecuteTask 是独立调用：
- 系统提示由 Executor 装配（assembler.rs：注入 role config + tools 白名单）
- 上下文完全从 ExecuteTaskParams 构建，不查白板
- 执行完成后 Executor 决定：等批次收集统一写 DB / 重试 / 放弃
- Sub-Agent 通过角色配置中的 `allowed_tools` 白名单使用 tools，各角色白名单不同
  - e.g. `coder` 可用：Bash / Read / Write / Edit / Glob / LSP
  - e.g. `planner` 可用：Read / Glob / WebSearch
  - Main Agent（Chat / Executor / Assistant / Evolver）同样各自拥有不同 tools 列表，按职责区分
- 沙箱隔离为 TODO（v0.1 不实现）

#### Worktree 与 Merge 锁

Sub-Agent 执行代码修改时，走 git worktree 隔离，**编辑阶段无冲突**。但 merge 回 main 时必须串行化。

**执行流程**：

```
Executor 分配 Task 给 Sub-Agent
→ Executor 为 Task 创建 worktree（task_id 命名）
→ Executor 通过 Server 记录 worktree metadata / Task 状态（Server 不创建 worktree）
→ Sub-Agent 在 worktree 内自由编辑（Bash/Read/Write/Edit/Glob）
→ Sub-Agent 完成后提交 worktree 的变更
→ Executor 决定：merge 回 main 还是放弃

Merge 阶段：
→ Executor 获取 Project 的 merge_lock（MongoDB 乐观锁）
→ 切换到 main worktree，执行 git merge {task_worktree}
→ 若冲突：LLM 辅助解决 → 重试 merge（最多 3 次）
→ 若仍失败：Task 标记 failed，释放 merge_lock
→ merge 成功：推送 main → 释放 merge_lock
```

**Merge 锁设计**：

```
Project 文档增加 merge_lock 字段；创建 Project 时必须写入 `merge_lock: { locked_by: null, locked_by_executor: null, locked_at: null }`，避免新 Project 缺字段导致锁获取条件不匹配：
{
"merge_lock": {
  "locked_by": "task_xxx",      // 当前持有锁的 Task ID
  "locked_at": ISODate(...),    // 锁获取时间
  "locked_by_executor": "exec-1" // 持有锁的 Executor
}
}

获取锁：db.projects.updateOne(
{ _id: project_id, "merge_lock.locked_by": null },
{ $set: { merge_lock: { locked_by: task_id, ... } } }
)
// matchedCount == 0 → 锁被占用，等待

释放锁：db.projects.updateOne(
{ _id: project_id, "merge_lock.locked_by": task_id },
{ $set: { "merge_lock.locked_by": null, "merge_lock.locked_by_executor": null, "merge_lock.locked_at": null } }
)
```

**Merge 锁 vs DAG depends_on**：

| 机制 | 作用 | 粒度 |
|------|------|------|
| `depends_on`（DAG） | 表达 Task 间的**逻辑依赖**（Task B 需要 Task A 的产出） | 业务层面 |
| `merge_lock` | 保证同一 main 分支的 merge 操作**物理串行** | git 层面 |

即使 DAG 表达了 depends_on，仍可能有多个并行 Task 同时完成并尝试 merge。Merge 锁保证同一时刻只有 1 个 Task 在 merge，避免 git 冲突。

**Executor 崩溃后的 Merge 锁释放**：
- Scheduler 检测 Executor 心跳超时 → 级联释放该 Executor 持有的所有 merge_lock

#### 影响范围
- `agents/src/sub_agent.rs` — Sub-Agent 实现（进程内调用）
- `agents/src/assembler.rs` — 装配器（role config + tools + 上下文注入）
- Sub-Agent 角色配置（TOML 中的 model / tools / system_prompt）
- `server/src/grpc/` 不需要 SubAgentExecutionService（v0.1 无 gRPC Sub-Agent）

#### 未决问题
- 无

#### 默认方案
- Sub-Agent 使用配置中声明的 allowed_tools
- project_context 由 Executor 用 LLM 摘要生成（包含 Project name + description + 已完成 Task 的关键产出）
- 沙箱隔离为后续 TODO

---

### 4. 故障恢复与 Checkpoint

#### 当前决策

```
恢复层级:
  1. Watch 断线    → resume_token 续传 → token 过期 → 全量扫描（周期性对账）
  2. Executor 崩溃 → Scheduler 心跳超时 → 释放 Project；仅将关联非终态 Task（InProgress/InReview/Retrying）回退 Pending → 分配给新 Executor
  3. Scheduler 崩溃 → 重启后全量扫描 assigned 超时、in_progress 且 assigned executor 心跳超时、以及 busy Executor 绑定异常 → 重建 Watch → 恢复调度
  4. API Server 崩溃 → 数据库已持久化所有状态 → 重启恢复监听 → Agent 自动重连

Scheduler 对账循环（每 60s）:
  - 扫描范围：assigned（超时）、in_progress（assigned executor 心跳超时）、busy Executor 但 current_project_id 不存在或已终态
  - assigned 超时的 Project → 清理分配信息并重置为 pending
  - in_progress 且 assigned executor 心跳超时的 Project → 按 Executor 崩溃恢复路径释放 Project、回退非终态 Task 并重置为 pending
  - busy Executor 但 current_project_id 不存在或指向 completed/failed/cancelled Project → 清理 Executor 绑定并回收/置 Idle
  - pending 超时且无 Executor → 扩展 Pool
  - 完成后写 checkpoint 到独立 `scheduler_state` collection（checkpoint_time, last_full_scan, processed_count）；`agent_heartbeats` 仅记录 Executor 心跳

Executor 崩溃后的完整恢复:
  1. Scheduler 检测 Executor 心跳超时（30s）
  2. 释放 Project：$unset assigned_executor_id + status→pending（仅崩溃恢复路径；普通执行失败不自动回退 Pending）
  3. 级联释放 ProjectTask：project_id 匹配 + status∈{in_progress,in_review,retrying} → pending + $unset assigned_executor_id
  4. 释放该 Executor 持有的所有 merge_lock：$set { "merge_lock.locked_by": null, "merge_lock.locked_by_executor": null, "merge_lock.locked_at": null }
  5. 新 Executor 分配后查询项目关联的 pending Task → 按编排策略重新执行
```

#### 影响范围
- `agent_heartbeats` collection 结构（仅 Executor 心跳）
- `scheduler_state` collection 结构（Scheduler 对账 checkpoint）
- Scheduler 核心逻辑（对账定时器、心跳检测）
- MongoDB Change Stream 的 resume_token 管理
- Agent 启动流程（断线重连 + 状态恢复）

#### 未决问题
- 无（已定稿：v0.1 单例 + crash-recovery，v0.2 主备；Watch 断线降级为对账全量扫描；Executor 崩溃级联释放 ProjectTask：仅将 InProgress/InReview/Retrying 回退 Pending；普通执行失败不自动回退 Pending）

#### 默认方案
- v0.1 Scheduler 单例 + crash-recovery（重启全量扫描），v0.2 考虑主备
- resume_token 过期 → 用 `db.collection.watch({ startAtOperationTime: last_known })` 替代，不可用时降级为全量扫描

---

### 5. 权限与 Workspace 隔离

#### 当前决策

```
两种 Token：

1. 用户 Token（账户系统签发，前端持有）:
   {
     "user_id": "uuid",
     "workspace_id": "uuid",
     "iat": ...,
     "exp": ...
   }
   用于 REST / WebSocket 请求，前端登录后获得。
   账户系统（登录 / 注册 / 会话管理）v0.1 简单实现，
   后续可扩为 OIDC。

2. Agent Token（API Server 签发，Agent 持有）:
   {
     "agent_id": "uuid",
     "role": "executor",         // 角色标识
     "workspace_id": "ws_uuid",  // 所属 Workspace
     "scope": ["board_read", "board_write"],
     "aud": "aemeath-api-server",
     "iss": "aemeath-api-server",
     "iat": ...,
     "exp": ...
   }
   用于 Agent ↔ API Server 的 gRPC 调用。
   API Server 在 Agent 注册成功后签发；服务端校验 Agent Token 时必须同时校验 `aud == "aemeath-api-server"` 与 `iss == "aemeath-api-server"`。

权限校验位置:
 1. API Server → 校验用户 Token（REST/WS）和 Agent Token（gRPC）；`scope` 控制 API Server 资源权限，如 `board_read` / `board_write` / `agent_registry`
 2. Agent 装配器 → 注入 token + 按 RoleConfig.allowed_tools 白名单过滤 Agent runtime 工具（如 Bash/Read/Write/WebSearch/Grep/Glob）
 3. Sub-Agent → 进程内 tokio task，**无 token**。权限由 Executor 的 `RoleConfig.allowed_tools` / `can_call_roles` 控制。DB 写入统一由 Executor 经 gRPC 调 Server。

`board_read` / `board_write` 属于 token `scope`，不属于 `allowed_tools`；`allowed_tools` 只描述 Agent runtime 可调用工具。Token scope 完全由 RoleConfig.scope 派生，签发时不做额外添加。

Workspace 隔离:
  - 所有 MongoDB 查询强制带 workspace_id filter（repository 层注入）
  - Sub-Agent 由 Executor 在进程内调用，workspace 上下文由 Executor 传递，无需独立校验
  - Workspace 之间数据完全隔离

Executor 产出写 DB，Server 聚合生成白板，Chat 感知汇报:
  - Executor 执行完成后写 project_result / project_task_result 到 DB
  - Server 自动聚合 DB 事实层 → 生成 / 更新 BoardSnapshot
  - Chat Watch BoardSnapshot + Requirement → 感知全局状态变化与需求状态变更 → 汇报用户
```

#### 影响范围
- 所有 gRPC Request message 需要 `workspace_id` 字段
- API Server 的 gRPC 拦截器链
- `agents/src/assembler.rs` 的 token 注入逻辑
- `repository/` 层所有查询（强制 workspace_id 添加）

#### 当前决策
- Token 签发：v0.1 由 API Server 直接签发。
- 租户 RBAC：v0.1 单租户。

#### 默认方案
- v0.1 API Server 自身签发 token（无独立 Auth Service），v0.2 可配置 OIDC
- v0.1 只支持个人 Workspace（is_personal=true），团队和 RBAC 放到 v0.2

---

### 6. 幂等与一致性策略

#### 当前决策

```
幂等 ID 存储:
  - MongoDB collection: idempotency_records
  - 文档结构: { _id: idempotency_key, endpoint, response, created_at }
  - response 仅缓存非敏感字段（project_id / status / task_id），不缓存 token / agent_id / workspace_id
  - TTL index: created_at_1, expireAfterSeconds=86400（24h）
  - 写入流程: 先尝试 insertOne({ _id: idempotency_key, endpoint, response, created_at })；若 unique index 冲突则返回已有记录的 response 字段
  - 重试: 相同 idempotency_key 的请求直接复用 idempotency_records.response 作为幂等响应

MongoDB ↔ Qdrant 一致性:
  - MongoDB 是主存储，Qdrant 是派生索引
  - 写入顺序: MongoDB → success → async task: Qdrant upsert
  - Qdrant 写失败: 设置 document.embedding_status = "failed"，后台 retry worker 周期扫描重试
  - Qdrant 不可用: embedding_status 保持 "pending"，Evolver 跳过 RAG，降级为规则化总结
  - 无分布式事务：不要求 MongoDB 和 Qdrant 原子性

版本冲突（乐观锁）:
  - 所有实体文档加 `version: u64` 字段
  - 更新操作: filter 包含 { _id, version: expected }, update 包含 { $inc: { version: 1 } }
  - 版本冲突 → 返回 ABORTED 错误 → 调用方重新读取最新文档 → 重试
  - 使用者: Scheduler（分配 Project）、Executor（完成 Task）、Assistant（更新 Requirement.draft，由 Chat 触发）
API 重试的重复创建防护:
  - Create 操作: idempotency_key + unique constraint（如 Chat.name + workspace_id 复合唯一索引）
  - Assign 操作: 条件更新（status + assigned_executor_id + version）天然幂等
  - 前端重试: 相同 idempotency_key 的 POST /api/.../chats/:chat_id/messages 返回已有 ChatMessage
```

#### 影响范围
- 所有 MongoDB entity document + `version` 字段
- `idempotency_records` collection + TTL 索引
- repository 层所有 update 方法签名（加上 expected_version 参数）
- gRPC 所有写操作的 request message（+ idempotency_key）
- Qdrant 的 embedding_status 字段 + 后台 retry worker

#### 未决问题
- 无（已定稿：TTL 24h，版本冲突固定 3 次重试 100ms 间隔，Qdrant 后台 worker 重试）

#### 默认方案
- TTL 24h，足够覆盖绝大多数场景；超时需要重建 idempotency_key
- 版本冲突重试：固定 3 次，每次间隔 100ms

---

### 7. 数据流设计（UI / Server / Agents）

#### 核心约束
- **Agents 不直接与 UI 交互**。所有 UI 可见状态由 Server 维护，UI 通过 Server 获取。
- **Server 是唯一的数据权威**（single source of truth）。Agents 不写 UI，只写 DB（通过 gRPC 调 Server）。

#### 数据流图

```
┌─────────────────────────────────────────────────────────┐
│                        UI（浏览器）                       │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐               │
│  │ 白板视图  │  │ 聊天视图  │  │ 需求视图  │               │
│  └────┬─────┘  └────┬─────┘  └────┬─────┘               │
│       │              │              │                     │
│       └──────────────┼──────────────┘                     │
│                      │                                    │
│          REST（查询）  │  WebSocket（推送）                 │
└──────────────────────┼────────────────────────────────────┘
                       │
┌──────────────────────┼────────────────────────────────────┐
│                   Server（API Server）                     │
│                      │                                    │
│  ┌───────────────────┼───────────────────┐               │
│  │     rest/         │     ws.rs         │               │
│  │  chat.rs          │  (状态推送)        │               │
│  │  requirement.rs   │                    │               │
│  │  project.rs       │                    │               │
│  │  board.rs         │                    │               │
│  └───────┬───────────┴───────┬───────────┘               │
│          │                   │                            │
│  ┌───────┴───────────────────┴───────────┐               │
│  │          repository/（MongoDB）         │               │
│  └───────────────────────────────────────┘               │
│          ▲                   ▲                   ▲        │
│          │ gRPC              │ gRPC              │ gRPC   │
└──────────┼───────────────────┼───────────────────┼────────┘
           │                   │                   │
 ┌─────────┴────────────────┐  ┌┴─────────────────┐  ┌┴────────────────┐
 │   Chat                   │  │   Scheduler      │  │   Executor      │
 │   (agents/)              │  │   (agents/)      │  │   (agents/)     │
 │ 接收用户消息             │  │ Watch DB 变更     │  │ 执行 ProjectTask│
 │ 写 ChatMessage           │  │ 分配 Project      │  │ 调用 SubAgent  │
 │ Chat Watch BoardSnapshot / 汇报用户 │  │ 调度 Assistant    │  │ 写 TaskResult  │
 └──────────────────────────┘            │
                         ┌───────┴────────────────────────────────────────────┐
                         │   Assistant                                        │
                         │   (agents/)                                        │
                         │ Assistant 分析 Requirements/Project（Scheduler 调度） │
                         │ Project/Task 草案                                  │
                         └────────────────────────────────────────────────────┘
                         ┌───────┴──────────┐
                         │   Evolver        │
                         │   (agents/)      │
                         │                  │
                         │   反思 + 优化     │
                         └──────────────────┘
```

#### 各层交互协议

| 方向 | 协议 | 用途 | 方向 |
|------|------|------|------|
| UI → Server | REST（HTTP） | 查询、CRUD 操作 | 请求-响应 |
| Server → UI | WebSocket | 状态变更实时推送 | 服务端推送 |
| Agents → Server | gRPC | 读写 DB、注册/心跳 | 请求-响应 |
| Server → Agents | gRPC | 指令下发（仅 Agent Registry） | 请求-响应 |

#### UI 的数据来源

| 数据类型 | 获取方式 | 更新方式 |
|----------|----------|----------|
| 白板（board） | REST GET `/board/{workspace_id}` | Server 聚合后 WebSocket 推送 |
| 需求列表 | REST GET `/requirements` | WebSocket 增量推送 |
| Project 状态 | REST GET `/projects/{id}` | WebSocket 推送 ProjectTask 完成 |
| 聊天消息 | REST GET `/chats/{id}/messages` | WebSocket 推送新消息 |
| Agent 状态 | REST GET `/api/workspaces/:ws_id/agents` | WebSocket 推送心跳/状态变更 |

#### 白板数据聚合（board.rs）

响应：`BoardSnapshot`，结构见 [Board 聚合响应结构](#board-聚合响应结构)。REST 初始全量拉取 `GET /api/workspaces/:ws_id/board` 默认限制 `recent_messages` 为最近 50 条，避免首次加载拉取过多历史消息；后续更新仍通过 WebSocket 推送增量 diff。

Server 收到 DB 变更（Agent 通过 gRPC 写入）后：
1. 聚合受影响的白板数据
2. 通过 WebSocket 向 UI 推送增量 diff

#### Agent 如何"呈现"信息

Agent **不直接写白板**。流程：

```
Executor 完成任务
  → gRPC 调 Server：UpdateProjectTaskResult(task_id, result)
  → Server 写 MongoDB：project_task.status = completed
  → Server 触发 board 聚合
  → Server WebSocket 推送 board diff 到 UI
  → UI 重新渲染
```

#### 数据层次（两层模型）

白板数据分两层，各司其职：

| 层次 | 数据 | 写入者 | 说明 |
|------|------|--------|------|
| **原始事实层** | Project / ProjectTask / ProjectResult / ProjectTaskResult | Executor（通过 gRPC） | Agent 执行的原始产出，写入 MongoDB |
| **展示视图层** | BoardSnapshot | Server（自动聚合） | 由 `board.rs` 从事实层实时计算，不依赖 Chat/Assistant 整理 |

```
Executor 写 TaskResult（事实层）
  → Server repository 写 MongoDB
  → Server board.rs 增量计算 BoardSnapshot（视图层）
  → WebSocket 推送 BoardSnapshot diff 到 UI
  → UI 直接渲染，无需二次处理
```

**Assistant 总结 ≠ 白板视图**：
- Assistant 完成需求分析后写的 `ProjectResult.summary` 是**事实层**的一条记录
- BoardSnapshot 包含 summary 字段，但它由 Server 聚合，不是 Assistant 直接生成
- WebSocket 推送的是 BoardSnapshot（视图层），不是单个 Task 的状态变更
- 前端始终 watch 聚合后的 BoardSnapshot，而非原始 Task 变更流

#### 未决问题
- 无（已定稿：Agents 不碰 UI，Server 是唯一数据权威，两层数据模型，WebSocket 推送 BoardSnapshot diff）

---

### 8. 日志系统设计

#### 设计原则
1. **三层独立日志**：UI / Server / Agents 各有自己的日志输出，互不干扰
2. **统一格式**：JSON Lines，每层加 `layer` 字段区分来源
3. **可聚合**：支持按 `trace_id` / `project_id` / `task_id` 串联三层日志
4. **Agent 审计日志**：所有 LLM 调用（请求/响应/token 用量）记录审计日志

#### 日志层级

| 层级 | 输出位置 | 包含内容 |
|------|----------|----------|
| UI | 浏览器 console + 可选远程上报 | 用户操作、前端错误、WS 断线 |
| Server | `~/.aemeath/server.log` | HTTP/gRPC 请求、DB 操作、白板聚合 |
| Agents | `~/.aemeath/agents/{role}.log` | Agent 生命周期、LLM 调用、任务执行 |

#### 统一日志格式（JSON Lines）

```jsonc
{
  "ts": "2026-05-17T10:30:00.123Z",   // ISO 8601 时间戳
  "layer": "server|agents|ui",         // 来源层
  "level": "INFO|WARN|ERROR|DEBUG",
  "module": "grpc::requirement",       // 模块路径
  "msg": "ProjectTask completed",      // 日志消息
  "trace_id": "req_abc123",            // 请求全链路追踪 ID
  "project_id": "proj_xyz",            // 可选：关联的业务 ID
  "task_id": "task_001",               // 可选
  "elapsed_ms": 1234,                  // 可选：耗时
  "extra": {}                          // 可选：上下文数据
}
```

#### trace_id 传递链

```
UI 发起请求（生成 trace_id）
  → HTTP Header: X-Trace-Id: xxx
    → Server 接收，注入到所有后续操作
      → gRPC Metadata: x-trace-id: xxx
        → Agent 接收，Logger 自动打 tag
```

全链路通过 `trace_id` 串联：`cat ~/.aemeath/*.log | jq 'select(.trace_id=="xxx")'` 即可看到完整调用链。

#### 审计日志（Agents）

| 文件 | 内容 |
|------|------|
| `~/.aemeath/audit/llm.log` | 每次 LLM 调用的请求/响应摘要、token 用量、耗时 |
| `~/.aemeath/audit/tool.log` | 每次工具调用的名称、参数摘要、结果大小、耗时、调用时的 `role` 与 `allowed_tools` 状态 |
| `~/.aemeath/audit/task.log` | ProjectTask 生命周期：pending → in_progress → completed/failed |

审计日志按天轮转：`llm.2026-05-17.log`。

Sub-Agent 工具调用审计：v0.1 信任 Executor 在进程内按 RoleConfig 过滤并执行工具调用，审计日志记录事后证据（每次调用时的 role、allowed_tools 快照、工具名和参数摘要）；v0.2 可考虑由服务端对工具调用事件做独立校验。

#### 日志级别配置

```toml
# ~/.aemeath/logging.toml
[server]
level = "info"
modules = { grpc = "debug", rest = "info", repository = "warn" }

[agents]
level = "info"
modules = { scheduler = "debug", executor = "debug", evolver = "info" }

[ui]
level = "warn"         # 前端默认不输出 debug 到 console
remote_report = false  # 可选远程上报
```

#### 影响范围
- `server/src/` — 引入 `tracing` crate，所有 gRPC/rest handler 加 span
- `agents/src/` — 引入 `tracing`，所有 Agent 逻辑加 span
- `ui/src/` — 前端日志封装，支持 console + 可选远程上报
- `share/` — 定义统一的 `LogRecord` 结构体（OpenAPI schema 的一部分）

#### 未决问题
- 无（已定稿：JSON Lines 统一格式，trace_id 全链路，三层独立输出）

---

### 9. 长任务超时 / 取消 / Token 刷新 / gRPC 错误码

#### 9.1 长任务超时与取消

`ExecuteTaskParams` 增加 deadline：

```rust
pub struct ExecuteTaskParams {
    // ... 已有字段
    pub deadline: Option<Instant>,       // 绝对截止时间（None = 无限）
}
```

Sub-Agent 执行循环中每条 LLM 调用前检查 deadline。超时后 Executor 将任务状态置为 `ProjectTaskStatus::Failed`；失败原因记录在 `ProjectTaskResult` 的 `error_message` / `output` 字段中。

**gRPC 取消服务**：

```protobuf
message CancelTaskRequest {
  string task_id = 1;
  string reason = 2;              // "user_cancelled" | "project_cancelled" | "timeout"
  string cancelled_by = 3;        // 取消发起方（user_id / agent_id）
}

message CancelTaskResponse {
  bool success = 1;
  TaskStatus previous_status = 2;
}
```

**级联取消**：
- 用户取消 Project → 级联取消所有 `status != completed/failed` 的 Task
- 用户取消 Requirement → 级联取消所有关联 Project → 再级联 Task

#### 9.2 Agent Token 刷新

Agent Token 有 `exp` 字段，长任务执行期间可能过期。

```protobuf
message RefreshTokenRequest {
  string agent_id = 1;
  // 服务端通过 gRPC metadata 中的 Agent Token 鉴权后刷新；request body 只传 agent_id。
}

message RefreshTokenResponse {
  string new_token = 1;           // 新 JWT（续期 exp）
  uint64 expires_in = 2;          // 秒
}
```

Executor 在执行长任务前检查 token 剩余有效期：
- `< 5min` → 主动 RefreshToken
- RefreshToken 失败 → 心跳也即将失败 → 提前标记 Project 为 pending 并退出

#### 9.3 gRPC 错误码

所有 gRPC Service 统一使用标准 gRPC Status Code：

| 错误码 | 触发条件 | 调用方处理 |
|--------|----------|-----------|
| `NOT_FOUND` | Project / Task / Requirement 不存在 | 重试无意义，返回 error |
| `PERMISSION_DENIED` | 调用方无权访问该资源 | 检查 Token 权限 |
| `FAILED_PRECONDITION` | 状态机不允许该操作（如 completed → in_progress） | 刷新状态后重试或忽略 |
| `ALREADY_EXISTS` | 创建请求的 idempotency_key 冲突 | 返回已有结果 |
| `ABORTED` | 乐观锁版本冲突 | 重试（带新 version） |
| `UNAVAILABLE` | 服务暂时不可用 | 指数退避重试 |
| `RESOURCE_EXHAUSTED` | Executor Pool 满 / 并发超限 | 等待后重试 |
| `DEADLINE_EXCEEDED` | 操作超时 | 检查超时设置，必要时重试 |

#### 9.4 `Register / Deregister` 硬校验

`RolePermissions.can_create_agents` 仅作为配置提示，**不作为最终授权来源**。

API Server 在 Agent 注册/注销 gRPC handler 中硬编码校验：
- 调用 `AgentRegistryService.Register / Deregister` 的请求方 `role` 必须为内置 `scheduler`
- 非 scheduler role 直接返回 `PERMISSION_DENIED`
- Scheduler Token 通过启动时配置预置密钥签发，不可被其他 Agent 获取

#### 影响范围
- `proto/project_task.proto` — CancelTask RPC
- `proto/agent.proto` — RefreshToken RPC + Register/Deregister 硬校验
- `agents/src/sub_agent.rs` — deadline 检查逻辑
- `server/src/grpc/` — 所有 handler 统一错误码
- `server/src/grpc/agent_registry.rs` — Register/Deregister 硬校验

#### 未决问题
- 无

---

### 10. 架构约束与 CI 门禁

#### 10.1 模块引用约束

```
规则 1（顶级隔离）:
  cli / server / agents / ui 之间不可直接引用，只能通过 share/ 层引用。

规则 2（server 内部隔离）:
  server/src/features/{A}/ 不可直接引用 server/src/features/{B}/，
  只能通过 server/src/share/ 中暴露的 trait 调用。

规则 3（agents 内部隔离）:
  agents/src/features/{role_A}/ 不可直接引用 agents/src/features/{role_B}/，
  只能通过 agents/src/share/ 中暴露的 trait 调用。
```

#### 10.2 CI Stop Hook（每次提交前强制执行）

```
Stop Hook 1 — 单元测试:
  cargo test --workspace
  必须全绿。

Stop Hook 2 — 顶级隔离检查:
  检查 cli / server / agents / ui 的 Cargo.toml 中 [dependencies]，
  禁止出现同级模块的直接引用。
  允许的引用：share/aemeath-core、share/aemeath-llm、share/aemeath-tools、share/proto、share/openapi。

Stop Hook 3 — server features 隔离检查:
  检查 server/src/features/{feature}/ 中所有 .rs 文件，
  禁止 use crate::features::{other_feature}:: 形式的跨 feature 引用。
  允许的引用：crate::share:: 下的 trait。

Stop Hook 4 — agents features 隔离检查:
  检查 agents/src/features/{role}/ 中所有 .rs 文件，
  禁止 use crate::features::{other_role}:: 形式的跨 role 引用。
  允许的引用：crate::share:: 下的 trait。
```

#### 10.3 影响范围
- `share/proto/` — CI 脚本：`buf lint` + `buf breaking`
- `Cargo.toml`（workspace 及各 crate）— Stop Hook 2 检查源
- `server/Cargo.toml` / `agents/Cargo.toml` — 不引入跨 feature 依赖

#### 未决问题
- 无

---

## 实施路线图

### Phase 1：基础设施（P0）
1. `proto/` — protobuf 定义（含 chat.proto / requirement.proto / project.proto）
2. `server/` — Server 骨架 + MongoDB 连接
3. gRPC CRUD Service 实现（Chat、Workspace、Requirement、Project、Task、Agent Registry）
4. MongoDB 索引创建（部分唯一索引、Change Streams 配置）
5. REST / WebSocket 网关实现（白板聚合接口 + 实时推送）
6. Watch 机制（MongoDB Change Streams → gRPC Server Streaming 桥接）
7. 故障恢复与幂等基础设施（heartbeat timeout、idempotency_key、Watch resume_token）

### Phase 2：Agent 运行时（P0）
1. `agents/src/template.rs` — 通用 Agent 模板
2. `agents/src/assembler.rs` — 装配器
3. Executor Pool 管理
4. 角色配置文件（chat / assistant / executor / evolver / scheduler / planner / coder / tester / reviewer / designer）

### Phase 3：调度与控制面（P1）
1. Scheduler 实现（Watch Project/Requirement + Executor/Assistant Pool 管理 + 扩缩）
2. Chat 实现（接收用户消息 → Watch 状态 → 汇报用户）
3. Assistant 实现（后台分析消息类型 → 拆解需求 → Project/Task 草案 → 确认收口）
4. Executor 实现（接收 Project → 编排 Task → 唤起 Sub-Agent → 结果写回白板 → 产出 summary）
5. Embedding 服务（异步生成 Requirement / Project / ProjectTask 向量，API Server 内异步 worker）

### Phase 4：前端（P1）
1. `ui/` — Vue 3 + Element Plus，Vite 构建，纯 Web
2. 原始需求、整理后的需求、Project & Task、Agent 状态四区块渲染
3. 用户确认草案的交互流程
4. 技术栈：Pinia 状态管理、Vue Router 路由、@tanstack/vue-query 数据获取
5. 开发服务器通过 CORS / proxy 指向 API Server
6. 数据获取走 `share/openapi/sdk/ts`（封装的 REST + WebSocket 客户端）

### Phase 5：CLI 集成（P2）
1. CLI 作为 API Server 的客户端
2. 保留现有单 Agent 模式作为轻量模式
3. `aemeath server` 子命令启动 API Server

### Phase 6：Evolver 与 RAG（P2）
1. Evolver 实现（定时扫描已完成 Project → Qdrant 检索相关上下文 → LLM 综合 → 产出模式总结 + Skills + MCP → 写入 reflections collection）
2. Qdrant collection 初始化、payload index 和后台重试 worker
3. 本地开发 fallback：未配置 Qdrant 时禁用 RAG，只保留规则化总结

## MVP 范围

Phase 1~3 完整实现风险过大。推荐分步交付：

### MVP v0.1 — 单 Agent + 白板（最小闭环）
- API Server + MongoDB + Chat / Requirement / Project / Task CRUD
- REST / WebSocket 简化版（轮询 + WS 推送）
- 单 Scheduler + 单 Executor（无 Pool）
- Assistant 只做需求写入和确认（不拆 Task）
- Evolver 延后
- 前端：Vue + Element Plus，只渲染白板四区块，不交互草案

### MVP v0.2 — 多 Agent 编排
- Executor Pool + 扩缩
- Assistant 分析 Requirement → 拆 Project/Task → 产出草案
- Executor 唤起 Sub-Agent（Planner + Coder + Tester）
- 前端：草案确认交互

### MVP v0.3 — 自我进化
- Evolver + RAG + 反思循环
- Skills / MCP 自动生成与注册
- 前端：白板自定义区块，DAG 可视化，Agent 状态面板

## P0 设计约束：故障恢复与幂等

### Executor 崩溃恢复
```
1. Scheduler 心跳超时检测（heartbeat_timeout_sec, 默认 30s）
2. 超时 Executor 的 current_project_id → 查 Project status:
   - in_progress → 仅在崩溃恢复路径清空 assigned_executor_id，并将 Project 回退 pending 以便重新分配
   - blocked     → Scheduler 通知 Chat，等待用户决策
   - completed/failed/cancelled → 终态不回退
3. Project 崩溃恢复条件更新（防并发）:
   db.projects.updateOne(
     { _id: project_id, status: { $in: ["pending", "in_progress"] }, assigned_executor_id: old_id },
     { $unset: { assigned_executor_id: "" }, $set: { status: "pending" } }
   )
4. 级联回退该 Project 下由该 Executor 持有的非终态 Task：
   InProgress / InReview / Retrying → Pending（清空 assigned_executor_id）
5. 重新分配给新 Executor 时写入原因字段（如 "reassigned_after_crash"）

约束：普通执行失败不会自动 Failed → Pending；只有 Executor 崩溃恢复回退非终态 Task，或显式人工重试/重开才会重新进入 Pending/分配流程。
```

### Task 重试
```
ProjectTask 状态:
  pending → in_progress → (completed | failed | retrying)
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

### Watch 断线恢复
```
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

## 关联

- 当前 Agent 系统：`aemeath-cli/src/agent_runner.rs`、`aemeath-tools/src/agent_tool.rs`
- 当前 Task 系统：`aemeath-core/src/task/`
- 当前配置系统：`aemeath-core/src/config/`
- 当前工具系统：`aemeath-core/src/tool.rs`、`aemeath-tools/`

## 开放问题

- 前端技术选型：**已定稿** — Vue 3 + Element Plus + Pinia + Vite（纯 Web，通过 `share/openapi/sdk/ts` 调 Server API）
- MongoDB Rust 驱动：官方 `mongodb` crate 还是 `bson` + 轻量封装？
- Scheduler 自身是否也在 Pool 中可多实例？还是严格单例？
- Embedding 模型选型：OpenAI text-embedding-3-small / 本地模型？
