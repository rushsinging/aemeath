# #36 多 Agent 框架 — Spec / 数据模型

## Session 与白板访问权限

### 角色分类

| 类型 | Agent | Session | 白板访问 | 说明 |
|---|---|---|---|---|
| Main Agent | Chat | 有 | 有 | 面向用户多轮对话，接收用户消息，Watch BoardSnapshot + Requirement 汇报用户 |
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
| Board 范围 | API Server gRPC 中间件 | Chat/Scheduler/Executor/Assistant/Evolver 的 token `scope` 包含 `board_read`/`board_write`；Main Agent 请求被 gRPC 中间件拦截（token 中携带 role/scope）；Sub-Agent 不直接连 API Server |
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
  "provider": "anthropic",
  "model": "claude-sonnet-4-20250514",
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
  "sender_id": ObjectId,         // 发送方 ID：用户 ID 或 Chat AgentInstance._id
  "role": "user",               // user | chat | system
  "content": "帮我做一个登录页面...",
  /*
   * message_type 由 Chat 分析后写入，完整枚举：question | requirement | feedback | clarification | chitchat
   *   question      - 用户简单提问，不产生 Requirement，不需要拆解
   *   requirement   - 用户提出可执行需求，1:1 关联 Requirement 文档
   *   feedback      - 用户对草案/执行结果的反馈或确认
   *   clarification - Agent 发起的澄清/追问，或用户对澄清问题的回答
   *   chitchat      - 闲聊、寒暄等非任务消息，不进入需求拆解
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
    "created_by": "assistant_agent_id"
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
    "locked_by": ObjectId,       //   当前持有锁的 AgentInstance._id；null = 未锁定（兼容锁获取条件）
    "locked_at": ISODate,        //   锁获取时间
    "locked_by_executor": ObjectId // 持有锁的 Executor AgentInstance._id
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

> v0.1 说明：`requires_approval` 审批字段/审批状态流暂不实现；Task 状态机不包含等待审批/审批通过/审批拒绝路径。后续版本若启用该字段，需要同步补充对应状态与转移规则。

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

## 关键数据结构

### Board 聚合响应结构
```rust
#[derive(Serialize)]
// Workspace 文档的核心子集（workspace_id / name / created_at / updated_at）
pub struct BoardSnapshot {
    pub snapshot_id: String,                     // 当前快照 ID，用于增量订阅一致性校验
    pub workspace: WorkspaceInfo,
    pub chats: Vec<Chat>,                         // Chat 会话
    pub recent_messages: Vec<ChatMessage>,        // 近期 Chat 消息（默认最近 50 条）
    pub requirements: Vec<Requirement>,           // Requirement 记录与草案
    pub projects: Vec<ProjectWithTasks>,           // Project & Tasks
    pub agent_instances: Vec<AgentInstance>,       // Agent 状态
}

#[derive(Serialize)]
pub struct BoardSnapshotUpdate {
    pub snapshot_id: String,        // 基于哪个快照做 diff
    pub timestamp: i64,
    pub changed_requirements: Vec<Requirement>,   // 新增/变更的 Requirement
    pub removed_requirement_ids: Vec<ObjectId>,   // 删除/移除的 Requirement ID
    pub changed_projects: Vec<ProjectWithTasks>,  // 新增/变更的 Project & Tasks
    pub removed_project_ids: Vec<ObjectId>,       // 删除/移除的 Project ID
    pub removed_task_ids: Vec<ObjectId>,          // 删除/移除的 ProjectTask ID
    pub changed_chats: Vec<Chat>,                 // 新增/变更的 Chat 会话
    pub new_messages: Vec<ChatMessage>,
    pub agent_status: Vec<AgentInstance>,
}

/// Workspace 文档的核心子集，嵌入 BoardSnapshot
pub struct WorkspaceInfo {
    pub name: String,
    pub provider: String,
    pub model: String,
    pub created_at: i64,
}

#[derive(Serialize)]
pub struct ProjectWithTasks {
    pub project: Project,
    pub tasks: Vec<ProjectTask>,
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

### Layer 3：成本分层

不同任务自动路由到不同 cost tier 的模型：

| 任务类型 | Cost Tier | 典型场景 |
|---|---|---|
| 代码生成 / 重构 / 复杂 review | High | Coder 核心任务、Reviewer 复杂审查 |
| 测试设计 / 测试执行 / 回归验证 | Medium | Tester 验证实现、生成测试报告 |
| UI/UX 设计 / 视觉方案 / 交互稿 | Medium | Designer 产出设计稿、交互建议 |
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
