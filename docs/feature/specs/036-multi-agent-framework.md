# #36 Multi-Agent 框架设计

## 概述

将当前单 Agent 架构升级为多 Agent 协作框架，参考 K8s 控制面设计：
- **API Server**（数据面）— gRPC（Agent 间通信）+ REST/WebSocket（前端），白板数据 CRUD + Watch
- **Scheduler**（调度面）— 管理 Agent Pool 生命周期，按需求量动态扩缩
- **Agent 角色**— 4 类 Main Agent（Assistant / Scheduler / Executor / Evolver）+ Sub-Agent（Executor 唤起，无状态）
- **分析/拆解**— Assistant 负责分析用户消息类型、拆解需求为 Project/Task、产出草案并交用户确认
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
| Session 策略 | 4 类 Main Agent（Assistant / Scheduler / Executor / Evolver）各自管理上下文；Sub-Agent 无状态，上下文由 Executor gRPC 传递 |
| 白板访问 | Assistant / Scheduler / Executor / Evolver 可访问；Sub-Agent 不可访问 |
| Agent 实现 | 一个通用模板 + 装配器（role → skill/MCP/prompt/权限），角色配置由 TOML 定义，支持用户自定义 Role |
| 需求分析 | Assistant 分析消息类型、拆解需求 → Project/Task，产出草案后交用户确认才写入 |
| Executor 分配策略 | Executor 按 Project 独占绑定，一个 Project 同时只分配一个 Executor |
| Evolver | 独立后台进程，定期扫描白板：总结已完成项目 → 提炼可复用模式 → 生成/优化 Skills、MCP 配置，驱动系统自我进化 |
| P0 约束 | MongoDB replica set（Change Streams 必需）；Change Streams 由 API Server 独占订阅，Agent / 前端通过 gRPC stream / WebSocket 间接消费 |
| MVP 交付 | v0.1 单 Agent + 白板 → v0.2 多 Agent 编排 → v0.3 自我进化（Evolver + RAG） |
| P0 故障恢复 | Executor 崩溃 → 心跳超时释放 Project；Task 重试 ≤3 次；gRPC 幂等写（idempotency_key）；Watch resume_token 断线续传 |

## 数据流

```
用户
 │
 ▼
Assistant ──(写原始需求)──▶  API Server ──▶  Mongo
 │                                       │
 │                                       ▼
 │                                 Render ◀── Mongo
 │                                       │
 │                                       ▼
 │                              白板（原始需求展示）
 │
 │ 分析消息类型、拆解需求（产出草案，待用户确认）
 │ 确认后写入
 ▼
API Server ──▶ Mongo ──▶ Render ──▶ 白板（Project + Project Task）

Scheduler ──Watch Project──▶ API Server
    │
    │ 管理 Pool，按 Project 分配 Executor
       ▼
     Executor ──▶  Sub-Agent（Planner / Coder / Tester / Reviewer / Designer）
       │               ↑
       │               │ 子 Agent 不访问白板，上下文由 Executor gRPC 传递
       │
       │ 写回 Project/Task 状态 → 白板变更
       │ Assistant Watch 白板 → 感知状态变化 → 汇报用户
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
                    白板 UI（纯前端，aemeath-frontend）
                         │  REST / WebSocket
                         ▼
┌──────────────────────────────────────────────────────────┐
│              API Server（aemeath-server）                  │
│                                                            │
│  ┌──────────────────┐   ┌──────────────────────────┐      │
│  │  REST / WS 网关   │   │   gRPC Service（Agent 间）│      │
│  │  （前端接口）      │   │                          │      │
│  └────────┬─────────┘   └──────────┬───────────────┘      │
│           │                        │                       │
│  ┌────────┴────────────────────────┴─────────────────┐    │
│  │ Chat Svc │ Requirement Svc │ Project Svc │ Task Svc │ Agent Reg │ │
│  └────────────────────────────────────────────────────┘    │
│                          │                                  │
│                    MongoDB（文档存储）                       │
└──────────────────────────────────────────────────────────┘
        ▲                      ▲
        │ gRPC Watch           │ gRPC Watch / CRUD
        │                      │
┌───────┴──────┐          ┌────┴──────────────────┐
│  Scheduler   │          │  Assistant（随连绑定）  │
│  （单例）     │          │  用户交互 + 分析需求   │
│              │          │  写白板 + 确认草案     │
│ Watch        │          │  汇报结果              │
│ Project      │          └───────────────────────┘
└──────┬───────┘
       │ 分派 Project (gRPC)
       ▼
┌─────────────────────┐
│  Executor Pool      │
│  #1, #2, ...        │
│  （持有 Session，   │
│   访问白板）         │
└──────────┬──────────┘
           │ 唤起 Sub-Agent（gRPC 传递上下文）
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
| Main Agent | Assistant | 有 | 有 | 面向用户多轮对话，分析消息类型、拆解需求、产出草案、确认 + 汇报 |
| Main Agent | Scheduler | 无（控制循环） | 有 | Watch + 分配决策，不参与对话 |
| Main Agent | Executor | 有 | 有 | 持有 Project 执行上下文，编排 Sub-Agent 执行 Tasks，有意义问题反馈 Assistant |
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
| Board 范围 | API Server gRPC 中间件 | Assistant/Executor/Evolver 有 read/write；Sub-Agent 请求被中间件拦截（token 中携带 role） |
| Tool allowlist | Agent 装配时注入 | 按 RoleConfig.permissions 的 `allowed_tools` 过滤 |
| Sub-Agent 调用 | Executor 端校验 | 按 RoleConfig.permissions 的 `can_call_roles` 限制可选角色 |
| 凭据隔离 | 装配时注入 | Sub-Agent 无 board 访问 token；Executor 不传递自身 credential |

## 白板渲染区域

| 区域 | 数据源 | 说明 |
|---|---|---|
| 原始需求 | Requirement（raw） | 用户通过 Assistant 提交的原始需求 |
| 整理好的需求 | Requirement（organized） | Assistant 分析整理后的结构化需求 |
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
  "updated_at": ISODate
}
```

### ChatMessage（会话消息）
```jsonc
{
  "_id": ObjectId,
  "chat_id": ObjectId,
  "workspace_id": ObjectId,
  "role": "user",               // user | assistant
  "content": "帮我做一个登录页面...",
  /*
   * message_type 由 Assistant 分析后写入：
   *   question     - 简单提问（不需要拆解）
   *   requirement  - 需求（需要 Assistant 分析拆解）
   *   clarification - 澄清/追问
   *   feedback     - 反馈/确认
   */
  "message_type": "requirement",
  /*
   * 关联的 Project 和 ProjectTask（多对多，数组存引用）
   * Assistant 分析后将 Message 关联到对应的 Project/Task
   */
  "project_ids": [ObjectId, ...],
  "task_ids": [ObjectId, ...],
  "metadata": {},                // 扩展字段
  "embedding_ref": {
    "collection": "chat_messages",
    "point_id": "<message_object_id>"
  },                              // Qdrant 引用（message_type=requirement 时有）
  "embedding_status": "pending", // pending | indexed | failed
  "created_at": ISODate
}
```

### Requirement
```jsonc
{
  "_id": ObjectId,
  "workspace_id": ObjectId,
  /*
   * source_message_ids — 来源 ChatMessage 引用
   * 一个需求可能由多条消息组合而成
   */
  "source_message_ids": [ObjectId],
  "title": "登录页面重构",
  "description": "需要重新设计登录页面...",
  "category": "raw",             // raw（原始） | organized（整理后）
  "status": "pending",           // pending | analyzing | draft | confirmed | in_progress | completed | rejected
  // draft — Assistant 产出草案（半自动，待确认）
  "draft": {
    "projects": [
      {
        "name": "前端登录页 UI",
        "tasks": [
          { "title": "设计登录页布局", "priority": 1 },
          { "title": "实现表单验证", "priority": 2 }
        ]
      }
    ],
    "summary": "该需求可拆解为 1 个前端 Project...",
    "created_by": "assistant_agent_id"
  },
  "created_at": ISODate,
  "updated_at": ISODate
}
```

### Project
```jsonc
{
  "_id": ObjectId,
  "workspace_id": ObjectId,
  "requirement_id": ObjectId,    // 关联的 Requirement
  /*
   * assigned_executor — 当前分配的 Executor Agent ID
   * 独占保证：Scheduler 事务 + AgentInstance.current_project_id 部分唯一索引，详见 Scheduler 独占分配机制
   */
  "assigned_executor": ObjectId,
  "name": "登录页 UI 重构",
  "status": "pending",           // pending | assigned | in_progress | blocked | failed | completed
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

### ProjectTask
```jsonc
{
  "_id": ObjectId,
  "project_id": ObjectId,
  "workspace_id": ObjectId,
  "title": "实现表单验证",
  "description": "需要支持邮箱格式校验...",
  "status": "pending",           // pending | assigned | in_progress | in_review | completed | failed
  "assigned_executor": ObjectId,       // 执行该 Task 的 Executor
  "priority": 1,
  /*
   * related_message_ids — 关联的 ChatMessage
   * 执行过程中产生的上下文消息（如：Executor 提问、Assistant 回复）
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

### AgentInstance
```jsonc
{
  "_id": ObjectId,
  "workspace_id": ObjectId,
  "role": "executor",            // 角色标识（内置 + 用户自定义）
  "role_config_ref": "roles/executor.toml",
  "status": "idle",              // idle | busy | error
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

### 数据关联总览（无外键，数组引用）
```
Chat  1:N  ChatMessage
ChatMessage  M:N  Project       (project_ids[])
ChatMessage  M:N  ProjectTask   (task_ids[])
Requirement  1:N  Project       (requirement_id)
Requirement  N:M  ChatMessage   (source_message_ids[])
Project  1:N  ProjectTask       (project_id)
Project  N:1  AgentInstance     (assigned_executor, 独占)
ProjectTask  N:1  AgentInstance  (assigned_executor)
ProjectTask  M:N  ChatMessage   (related_message_ids[])
```

## 关键数据结构

### MessageType（ChatMessage 类型枚举）
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MessageType {
    Question,        // 简单提问，不需要拆解
    Requirement,     // 需求，需 Assistant 分析拆解
    Clarification,   // 澄清/追问
    Feedback,        // 反馈/确认
}
```

### RequirementStatus（Requirement 状态枚举）
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RequirementStatus {
    Pending,        // 待分析（用户刚提交的原始需求）
    Analyzing,      // Assistant 正在分析中
    Draft,          // 草案已产出，等待用户确认
    Confirmed,      // 用户已确认，等待创建 Project
    InProgress,     // 关联的 Project 正在执行中
    Completed,      // 所有关联 Project 已完成
    Rejected,       // 用户驳回草案
}
```

### ProjectStatus（Project 状态枚举）
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ProjectStatus {
    Pending,        // 待 Scheduler 分配 Executor
    Assigned,       // 已分配 Executor，等待 Executor 接受
    InProgress,     // Executor 已接受并正在执行
    Blocked,        // 等待用户反馈或外部依赖
    Failed,         // 执行失败，等待重试或人工处理
    Completed,      // 全部 ProjectTask 完成
}
```

### ProjectTaskStatus（ProjectTask 状态枚举）
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ProjectTaskStatus {
    Pending,        // 待分配（等待 Scheduler 分派给 Executor）
    Assigned,       // 已分配给 Executor
    InProgress,     // Executor 正在执行
    InReview,       // 进入 Review 阶段
    Completed,      // 执行成功
    Failed,         // 执行失败，需重新分派
}
```

### AgentStatus（AgentInstance 状态枚举）
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AgentStatus {
    Idle,           // 空闲，可接收新任务
    Busy,           // 正在执行任务
    Error,          // 异常状态，需 Scheduler 介入
}
```

### AgentRole（动态角色标识，支持内置 + 用户自定义）

角色不是固定枚举，而是字符串标识 + 角色配置文件。Rust 侧保留字符串常量引用：

```rust
/// 内置角色常量（代码引用用）
pub mod builtin_roles {
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
    pub permissions: RolePermissions,
    pub skills: Vec<String>,
    pub mcp: McpConfig,
    // 用户自定义角色可额外扩展字段
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RolePermissions {
    pub allowed_tools: Vec<String>,
    pub max_subagents: usize,
    pub can_call_roles: Vec<String>,
    pub can_create_agents: bool,
}
```

用户自定义角色：在 `roles/` 目录下新增 TOML 文件，Scheduler 启动时扫描加载。例如：

```toml
# roles/security-auditor.toml（用户自定义）
[role]
name = "security-auditor"
description = "安全审计 Agent"
[[models]]
model = "anthropic/claude-4-sonnet-20250514"
cost_tier = "high"

[permissions]
allowed_tools = ["board_read", "board_write", "bash", "grep"]
max_subagents = 0
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
| 架构设计 / 拆解需求 / 产出草案 | Medium | Planner、Assistant 分析 |
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

```jsonc
// MongoDB 中独立 collection: model_health
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

Scheduler 也可以通过 Watch model_health 在全局层面预判模型可用性，避免大量 Agent 同时撞到同一不健康的模型。

## 状态流转

### Requirement 状态流转
```
用户提交需求
     │
     ▼
  Pending ──────────────────────────────────────────────┐
     │                                                   │
     │ Assistant 收到用户请求，开始分析                      │
     ▼                                                   │
  Analyzing ──(分析完成)──▶ Draft ──(用户确认)──▶ Confirmed
     │                        │                            
     │                        │ (用户驳回)                  
     │                        ▼                            
     │                     Rejected ──(用户重新提交)──▶ Pending
     │                                                   
     │ (分析失败/超时)                                    
     ▼                                                   
  Pending（重新进入队列等待 Assistant）                    
                                │                        
                          Confirmed ──(创建 Project)──▶ InProgress
                                                            │
                                      (所有 Project 完成) ──▶ Completed
```

### ProjectTask 状态流转
```
Scheduler Watch 到 Pending Project
     │
     ▼
  Pending ──(分配 Executor)──▶ Assigned
                            │
                            │ Executor 开始执行
                            ▼
                        InProgress ──(子任务完成，进入 Review)──▶ InReview
                            │                                       │
                            │                                       │ Review 通过
                            │                                       ▼
                            │                                   Completed
                            │                                       │
                            │ (执行失败)                             │
                            ▼                                       │
                          Failed ──(Scheduler 重新分派)──▶ Pending
```

### AgentInstance 生命周期
```
Scheduler 创建 Agent
     │
     ▼
   Idle ──(领取任务)──▶ Busy
     ▲                    │
     │                    │ 任务完成
     │                    ▼
     │                  Idle
     │
     │ (心跳超时)
     ▼
   Error ──(Scheduler 回收)──▶ 销毁
```

### Scheduler 调度决策流程
```
Scheduler Watch 循环:

  Project 变更事件（status=pending 且无 assigned_executor）:
    │
    ├── Executor Pool 未达 max → 创建新 Executor 实例，分配 Project
    │
    └── Executor Pool 已达 max → Project 保持 Pending，等待下次 Watch

  实例空闲 > scale_down_idle_secs 且 当前数 > min:
    └── 回收实例（Deregister + 通知退出）
```

## Agent 生命周期管控

Scheduler 只管理 Executor Pool。Assistant 和 Evolver 独立管控，不与 Scheduler 耦合。

| Agent | 模式 | 生命周期 | 管控者 |
|---|---|---|---|
| **Executor** | Pool（动态伸缩） | Scheduler 按 pending Project 数创建/销毁 | Scheduler |
| **Assistant** | 随连绑定（无 Pool） | 用户连接 → 创建/复用；断连 + 超时 → 释放 | 连接层 |
| **Scheduler** | 单例 | 启动即注册，常驻 | — |
| **Evolver** | 单例 | 启动即注册，常驻后台，定时循环 | 自身 |

```
Assistant —── 随连绑定（无 Pool）───
  Workspace 有用户连接 → 创建 / 复用 Assistant
  用户断连 + 超时       → 释放资源

Evolver —── 单例 + 定时器 ───
  启动时注册为单例
  循环: sleep(interval) → 扫描已完成 Project → 提炼模式 → 产出 Skills/MCP → sleep(interval)

Executor —── Pool（Scheduler Watch 驱动）───
  Project pending → Scheduler 创建 Executor → 绑定分配
  Project 完成 / 空闲超时 → Scheduler 销毁
```

### 管控边界
- **Scheduler** 是唯一能做 Agent 创建/销毁动态决策的组件，但只管辖 Executor Pool
- **Assistant** 生命周期由连接层（gRPC session）管理，Scheduler 不参与
- **Evolver** 自身维护定时循环，不受 Scheduler 调度

### 模型健康状态流转
```
Healthy ──(请求失败)──▶ Degraded ──(再次失败)──▶ Degraded ──(第3次失败)──▶ Unhealthy
   ▲                       │                                                  │
   │                       │ (成功)                                           │
   │                       ▼                                                  │
   └────────────────── Healthy ◀────────(冷却 60s 后重试成功)─────────────────┘

降级路径（按 RoleModelConfig 列表优先级）:
  同 tier 逐项降级 → 下一 tier（high → medium → low）→ 全部 Unhealthy → Scheduler 告警
```

## Scheduler 设计

### 职责
- 管理 Executor Pool（Assistant 和 Evolver 不受 Scheduler 管辖）
- 根据 pending Project 数量动态创建/销毁 Executor 实例
- **将 Project 分派给 idle 状态的 Executor**（Project 粒度分配，非 Task 粒度）
- 分配前检查目标 Project 是否已有 assigned_executor，防止两个 Executor 分配到同一 Project

### Project 独占分配机制
```
分配前提:
  - Project._id 天然保证单文档原子更新（_id 唯一）
  - AgentInstance.current_project_id 部分唯一索引用于防止多个 Executor 同时声明同一个 Project
  - 单个 Executor 同时只能处理一个 Project，由 AgentInstance 单文档字段和状态机保证

分配流程（必须在 MongoDB transaction 中执行）:
  1. 条件更新 Project:
     db.projects.updateOne(
       { _id: project_id, assigned_executor: { $exists: false }, status: "pending" },
       { $set: { assigned_executor: executor_id, status: "assigned" } }
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
  - 不需要在 Project.assigned_executor 上建唯一索引（_id + 条件更新 + transaction 已保证 Project 分配互斥）
```

### 扩缩策略

Scheduler 只管理 Executor Pool。Assistant 随用户连接绑定，Evolver 为单例：

```
Executor Pool 大小 = f(pending_project_count, max_concurrent_executor)
```

每个 Pool 有 min/max 实例限制，按需在区间内自动伸缩。

### 调度流程
```
Scheduler Watch 循环:
  1. 收到 Project 变更事件（status=pending 且无 assigned_executor）
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
| ChatMessage（类型=requirement） | 需求文本提炼（LLM 将长文本压缩为 512 token 摘要后向量化） | 消息写入时 |
| Project（状态=completed） | project name + 生成的 summary + 关键决策描述 | Project 完成时 |
| ProjectTask（状态=completed） | task name + Executor 产出摘要 + 遇到的坑/解法 | Task 完成时 |

### 反思流程
```
Evolver 定时循环（interval 可配，默认 24h）:

1. 扫描近期（~7d）已完成但未反思的 Project
2. 对每个 Project 做 embedding 检索:
   - 用"有哪些之前做过的类似项目？"查询 Project embedding
   - 用"有哪些反复出现的问题？"查询 ProjectTask embedding
   - 用"用户最近关注什么方向？"查询 ChatMessage embedding
3. LLM 综合检索到的上下文，产出:
   a. 模式总结（如"XXX 类需求推荐用 YYY 技术方案"）
   b. Skill 生成/优化（如自动生成 react-form 模板 Skill）
   c. MCP 建议（如检测到重复 API 调用 → 建议配置对应 MCP server）
4. 反思结果写入白板（reflections collection）
5. 标记 Project 为已反思
```

### Qdrant RAG 存储边界

RAG 不属于 P0 的硬依赖。向量数据统一存入 Qdrant，MongoDB 仅保存业务数据和向量引用：
- Qdrant collections：`chat_messages`、`projects`、`project_tasks`。
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
// same shape for chat_messages, projects

```

## Agent 实现：模板 + 装配器

### 通用模板（`aemeath-agents/template/`）
Agent 运行时核心：
- LLM 对话循环（当前 agent_runner 提取/抽象）
- 工具调用执行
- 上下文管理（压缩、token 估算）
- 结果汇总

### 装配器（`aemeath-agents/assembler.rs`）
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

### 角色配置（`aemeath-agents/roles/`）

```toml
# assistant.toml
[role]
name = "assistant"
description = "用户交互层，将用户意图写入白板"
[[models]]
model = "anthropic/claude-4-sonnet-20250514"
cost_tier = "high"

[permissions]
allowed_tools = ["board_write", "board_read"]
max_subagents = 0

[skills]
enabled = []

[mcp]
servers = []
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
allowed_tools = ["board_read", "board_write", "web_search"]
max_subagents = 0

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
allowed_tools = ["agent_registry", "board_read", "board_write"]
can_create_agents = true
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
allowed_tools = ["board_read", "board_write", "agent_call"]
max_subagents = 5
can_call_roles = ["planner", "coder", "tester", "reviewer", "designer"]

[skills]
enabled = ["task-management"]

[mcp]
servers = []
```

## 项目结构变更

### Crate 依赖关系
```
aemeath-frontend/     # 纯 UI ──HTTP/WS──▶ aemeath-server
aemeath-server/       # gRPC + REST/WS ──依赖──▶ aemeath-core
aemeath-agents/       # Agent 运行时 ──依赖──▶ aemeath-core, aemeath-llm
                      #     ──gRPC──▶ aemeath-server (Watch + CRUD)
aemeath-cli/          # 保留 ──依赖──▶ aemeath-core, aemeath-llm, aemeath-tools
aemeath-tools/        # 不变 ──依赖──▶ aemeath-core
aemeath-llm/          # 不变 ──依赖──▶ aemeath-core
aemeath-core/         # 不变（共享核心库）
proto/                # protobuf 定义，生成 Rust 代码给 server 和 agents 用
```

### 目录结构

```
aemeath/                      # workspace root
├── aemeath-core/             # 核心库（不变）
├── aemeath-llm/              # LLM 客户端（不变）
├── aemeath-tools/            # 工具注册（不变）
├── aemeath-cli/              # CLI 入口（保留）
├── aemeath-server/           # ★ 新增：API Server
│   ├── Cargo.toml
│   ├── src/
│   │   ├── main.rs           #   服务入口
│   │   ├── grpc/             #   gRPC Service（Agent 间通信）
│   │   │   ├── chat.rs
│   │   │   ├── workspace.rs
│   │   │   ├── requirement.rs
│   │   │   ├── project.rs
│   │   │   ├── project_task.rs
│   │   │   └── agent_registry.rs
│   │   ├── rest/             #   REST / WebSocket 网关（前端接口）
│   │   │   ├── mod.rs
│   │   │   ├── board.rs      #   白板数据聚合接口
│   │   │   └── ws.rs         #   WebSocket 推送
│   │   ├── repository/       #   MongoDB 数据访问层（trait 抽象）
│   │   │   ├── mod.rs
│   │   │   ├── workspace.rs
│   │   │   ├── chat.rs
│   │   │   ├── requirement.rs
│   │   │   ├── project.rs
│   │   │   └── agent.rs
├── aemeath-frontend/         # ★ 新增：纯 UI 前端（白板渲染）
│   └── src/                  #   （通过 REST/WS 调 Server）
├── aemeath-agents/           # ★ 新增：Agent 运行时
│   ├── Cargo.toml
│   ├── src/
│   │   ├── lib.rs
│   │   ├── template.rs       #   通用 Agent 模板
│   │   ├── assembler.rs      #   装配器
│   │   └── pool.rs           #   Agent Pool 管理
│   └── roles/                #   角色配置
│       ├── assistant.toml     #   内置角色
│       ├── scheduler.toml
│       ├── executor.toml
│       ├── evolver.toml
│       ├── planner.toml
│       ├── coder.toml
│       ├── tester.toml
│       ├── reviewer.toml
│       ├── designer.toml
│       └── custom/            #   用户自定义角色（Scheduler 扫描加载）
├── proto/                    # ★ 新增：protobuf 定义
│   ├── chat.proto
│   ├── workspace.proto
│   ├── requirement.proto
│   ├── project.proto
│   ├── agent.proto
│   └── common.proto
├── docs/
└── CLAUDE.md
```

## REST / WebSocket API 设计（前端接口）

Server 通过 REST + WebSocket 为前端白板提供数据。

### REST 端点
```
GET    /api/workspaces/:ws_id/board              # 白板聚合数据（一次性返回全部区块）
GET    /api/workspaces/:ws_id/requirements       # 需求列表（支持 ?category=raw&status=pending 过滤）
GET    /api/workspaces/:ws_id/projects           # Project 列表（支持 ?status=in_progress 过滤）
GET    /api/workspaces/:ws_id/projects/:id/tasks # 某个 Project 的 Task 列表
GET    /api/workspaces/:ws_id/agents             # Agent 实例列表
POST   /api/workspaces/:ws_id/requirements       # 创建需求（Assistant 调用）
POST   /api/workspaces/:ws_id/requirements/:id/confirm  # 确认草案
POST   /api/workspaces/:ws_id/requirements/:id/reject   # 驳回草案
```

### WebSocket
```
WS /ws/workspaces/:ws_id/board
  → 实时推送白板数据变更事件：
    - requirement_updated
    - project_created
    - task_status_changed
    - agent_status_changed
```

### Board 聚合响应结构
```rust
#[derive(Serialize)]
pub struct BoardSnapshot {
    pub workspace: WorkspaceInfo,
    pub raw_requirements: Vec<Requirement>,       // 原始需求
    pub organized_requirements: Vec<Requirement>, // 整理好的需求
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

### Chat Service
```protobuf
service ChatService {
  rpc Create(CreateChatRequest) returns (Chat);
  rpc AddMessage(AddMessageRequest) returns (ChatMessage);
  rpc AnalyzeMessage(AnalyzeMessageRequest) returns (ChatMessage);  // Assistant 分析消息类型 + 关联
  rpc Get(GetChatRequest) returns (Chat);
  rpc List(ListChatsRequest) returns (ListChatsResponse);
  rpc Watch(WatchRequest) returns (stream ChatEvent);
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
```

### Requirement Service
```protobuf
service RequirementService {
  rpc Create(CreateRequirementRequest) returns (Requirement);
  rpc Update(UpdateRequirementRequest) returns (Requirement);
  rpc Get(GetRequirementRequest) returns (Requirement);
  rpc List(ListRequirementsRequest) returns (ListRequirementsResponse);
  rpc Watch(WatchRequest) returns (stream RequirementEvent);
  rpc ConfirmDraft(ConfirmDraftRequest) returns (Requirement);  // 用户确认草案
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
  rpc Complete(CompleteProjectRequest) returns (Project);  // Executor 标记 Project 完成
  rpc Watch(WatchRequest) returns (stream ProjectEvent);
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
}
```

## 实施路线图

### Phase 1：基础设施（P0）
1. `proto/` — protobuf 定义（含 chat.proto / project.proto）
2. `aemeath-server/` — Server 骨架 + MongoDB 连接
3. gRPC CRUD Service 实现（Chat、Workspace、Requirement、Project、Task、Agent Registry）
4. MongoDB 索引创建（部分唯一索引、Change Streams 配置）
5. REST / WebSocket 网关实现（白板聚合接口 + 实时推送）
6. Watch 机制（MongoDB Change Streams → gRPC Server Streaming 桥接）
7. 故障恢复与幂等基础设施（heartbeat timeout、idempotency_key、Watch resume_token）

### Phase 2：Agent 运行时（P0）
1. `aemeath-agents/template.rs` — 通用 Agent 模板
2. `aemeath-agents/assembler.rs` — 装配器
3. Executor Pool 管理
4. 角色配置文件（executor / evolver / assistant / scheduler / planner / coder / tester / reviewer / designer）

### Phase 3：调度与控制面（P1）
1. Scheduler 实现（Watch Project + Executor Pool 管理 + 扩缩）
2. Assistant 实现（分析消息类型 → 拆解需求 → Project/Task 草案 → 确认收口 → Watch 状态 → 汇报用户）
3. Executor 实现（接收 Project → 编排 Task → 唤起 Sub-Agent → 结果写回白板 → 产出 summary）
4. Embedding 服务（异步生成 ChatMessage / Project / ProjectTask 向量，API Server 内异步 worker）

### Phase 4：前端（P1）
1. `aemeath-frontend/` — 白板 UI
2. 原始需求、整理后的需求、Project & Task、Agent 状态四区块渲染
3. 用户确认草案的交互流程

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
- API Server + MongoDB + Chat / Requirement CRUD
- REST / WebSocket 简化版（轮询 + WS 推送）
- 单 Scheduler + 单 Executor（无 Pool）
- Assistant 只做需求写入和确认（不拆 Task）
- Evolver 延后
- 前端：只渲染白板四区块，不交互草案

### MVP v0.2 — 多 Agent 编排
- Executor Pool + 扩缩
- Assistant 分析需求 → 拆 Project/Task → 产出草案
- Executor 唤起 Sub-Agent（Planner + Coder + Tester）
- 前端：草案确认交互

### MVP v0.3 — 自我进化
- Evolver + RAG + 反思循环
- Skills / MCP 自动生成与注册
- 前端：白板自定义区块

## P0 设计约束：故障恢复与幂等

### Executor 崩溃恢复
```
1. Scheduler 心跳超时检测（heartbeat_timeout_sec, 默认 30s）
2. 超时 Executor 的 current_project_id → 查 Project status:
   - in_progress → 保持 in_progress，Scheduler 重新分配（清空 assigned_executor）
   - blocked     → Scheduler 通知 Assistant，等待用户决策
3. Project 重新分配条件更新（防并发）:
   db.projects.updateOne(
     { _id: project_id, status: { $in: ["pending", "in_progress"] }, assigned_executor: old_id },
     { $unset: { assigned_executor: "" }, $set: { status: "pending" } }
   )
4. 重分配给新 Executor 时写入原因字段（如 "reassigned_after_crash"）
```

### Task 重试
```
ProjectTask 状态:
  pending → in_progress → (completed | failed | retrying)

Executor 重启后:
  1. 查询自身 assigned_task（status=in_progress）
  2. 从 Mongo 加载 Task 上下文（description + related_message_ids）
  3. 重新执行
  4. 连续失败 ≥3 → status=failed → 通知 Assistant

重试携带 retry_count + last_error，Sub-Agent 可根据失败历史调整策略。
```

### 幂等设计
```
gRPC 所有写操作携带 idempotency_key（UUID）：

  AssignProjectRequest  { idempotency_key, project_id, executor_id }
  CompleteTaskRequest    { idempotency_key, task_id }

Server 端:
  - 每个 collection 建 idempotency_keys 子文档
  - 收到请求 → 查缓存：命中 → 返回上次结果；未命中 → 执行 → 缓存结果 + idempotency_key
  - 缓存 TTL 24h，防止内存泄漏
```

### Watch 断线恢复
```
- 客户端保存最后一个 change event 的 resume_token
- 断线后从 resume_token 恢复，避免重复消费
- 指数退避重连：1s → 2s → 4s → ... → max 60s
- 白板 UI 显示连接状态（disconnected / reconnecting / connected）
```

## 关联

- 当前 Agent 系统：`aemeath-cli/src/agent_runner.rs`、`aemeath-tools/src/agent_tool.rs`
- 当前 Task 系统：`aemeath-core/src/task/`
- 当前配置系统：`aemeath-core/src/config/`
- 当前工具系统：`aemeath-core/src/tool.rs`、`aemeath-tools/`

## 开放问题

- 前端技术选型：Tauri + Dioxus / egui / Web（Tauri + React）？
- MongoDB Rust 驱动：官方 `mongodb` crate 还是 `bson` + 轻量封装？
- Agent 实例是独立进程还是 tokio task？
- Scheduler 自身是否也在 Pool 中可多实例？还是严格单例？
- MongoDB Atlas 本地开发替代：`mongod --replSet rs0` 单节点 replica set 可满足 Change Streams
- Embedding 模型选型：OpenAI text-embedding-3-small / 本地模型？
