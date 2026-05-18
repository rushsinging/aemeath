# #36 多 Agent 框架 — Spec / 架构与 Agent 设计

## 概述

将当前单 Agent 架构升级为多 Agent 协作框架，参考 K8s 控制面设计：
- **API Server**（数据面）— gRPC（Agent 间通信）+ REST/WebSocket（前端），白板数据 CRUD + Watch
- **Scheduler**（调度面）— 管理 Agent Pool 生命周期，按需求量动态扩缩
- **Agent 角色**— 5 类 Main Agent（Chat / Scheduler / Executor / Assistant / Evolver）+ Sub-Agent（Executor 唤起，无状态）
- **对话/分析拆解**— Chat 分析消息类型，Scheduler 调度 Assistant 做需求深度拆解为 Project/Task，草案存入 `Requirement.draft`；Chat 面向用户多轮对话并汇报状态
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
| Session 策略 | 5 类 Main Agent（Chat / Scheduler / Executor / Assistant / Evolver）各自管理上下文；Sub-Agent 无状态，上下文由 Executor 进程内调用传递 |
| 白板访问 | Chat / Scheduler / Executor / Assistant / Evolver 可访问；Sub-Agent 不可访问 |
| Agent 实现 | 一个通用模板 + 装配器（role → skill/MCP/prompt/权限），角色配置由 TOML 定义，支持用户自定义 Role |
| 需求分析 | Chat 接收用户消息 + 分析消息类型；Scheduler 调度 Assistant 做需求深度拆解 → Project/Task，草案存入 `Requirement.draft`，用户确认后写入 Project/Task |
| Executor 分配策略 | Executor 按 Project 独占绑定，一个 Project 同时只分配一个 Executor |
| Evolver | 独立后台进程，定期扫描白板：总结已完成项目 → 提炼可复用模式 → 生成/优化 Skills、MCP 配置，驱动系统自我进化 |
| P0 约束 | MongoDB replica set（Change Streams 必需）；Change Streams 由 API Server 独占订阅，Agent / 前端通过 gRPC stream / WebSocket 间接消费 |
| MVP 交付 | v0.1 单 Agent + 白板 → v0.2 多 Agent 编排 → v0.3 自我进化（Evolver + RAG） |
| P0 故障恢复 | Executor 崩溃 → 心跳超时释放 Project，并仅在崩溃恢复时将非终态 Task（InProgress/InReview/Retrying）回退 Pending；Task 重试 ≤3 次；gRPC 幂等写（idempotency_key）；Watch resume_token 断线续传 |


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
       │
       ├── 创建/调度 Assistant Pool 执行 Requirement 分析 ──► ┌─────────────────────┐
       │                                                   │  Assistant Pool     │
       │                                                   │  后台分析/拆解/汇总   │
       │                                                   └─────────────────────┘
       │ 分派 Project (gRPC)
       ▼
┌─────────────────────┐
│  Executor Pool      │
│  #1, #2, ...        │
│  （持有 Session，   │
│   访问白板）         │
└──────────┬──────────┘
           │ 唤起 Sub-Agent（进程内调用；Sub-Agent 与 Executor 同进程，异步调用）
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
             │ gRPC Watch / CRUD
             └───────────────────────────────► API Server
```


### AgentRole（动态角色标识，支持内置 + 用户自定义）

角色不分固定枚举，而是字符串标识 + 角色配置文件。内置角色按生命周期分为两组：

| Main Agent（长期运行） | Sub-Agent（按需唤起） | 调用发起方 / 实际调度关系 |
|---|---|---|
| chat | - | 连接层按用户连接创建/复用 |
| assistant | planner | Scheduler 调度 Assistant；Executor 进程内异步调用 planner |
| scheduler | coder | Scheduler 自身单例；Executor 进程内异步调用 coder |
| executor | tester | Scheduler 创建/分派 Executor；Executor 进程内异步调用 tester |
| evolver | reviewer | Evolver 自身单例；Executor 进程内异步调用 reviewer |
|  | designer | Executor 进程内异步调用 designer |

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
    pub pool_size: usize,                 // Pool 期望实例数；0 表示无 Pool/随连绑定或单例
    pub system_prompt: String,
    pub models: Vec<RoleModelConfig>,    // 模型列表，按优先级排列
    pub permissions: RolePermissions,     // Agent runtime 工具权限；API Server 资源权限由 token scope 控制
    pub skills: Vec<String>,
    pub mcp: McpConfig,                 // 继承现有 McpConfig 定义
    // 用户自定义角色可额外扩展字段
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

fn default_max_subagents() -> usize {
    0
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
  2. Watch Requirement（status=pending → 查询空闲 Assistant → 分配分析任务）
  3. 查询空闲 Executor
  4. 无空闲 Executor 且未达 max → 创建新 Executor
  5. 在 MongoDB transaction 中条件绑定 Project → Executor
  6. 更新 Project status → assigned
  7. 更新 Executor status → busy
  8. Executor 接受分配后调用 ProjectService.Accept，Project status → in_progress
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
- Qdrant collections：`chat_messages`、`requirements`、`projects`、`project_tasks`、`reflections`（同 shape，但不同 collection 可配置不同向量维度和 payload index；`reflections` 对应 reflections schema 中 `embedding_ref.collection = "reflections"`）。
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

// same shape for chat_messages, requirements, projects, reflections


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
[role]
name = "chat"
description = "面向用户的对话 Agent"
pool_size = 0               # 随连绑定，无 Pool

[[models]]
model = "anthropic/claude-sonnet-4-20250514"
cost_tier = "high"

[permissions]
allowed_tools = ["read", "write", "web_search", "web_fetch"]
scope = ["board_read", "board_write"]
can_call_roles = ["scheduler"]  # Chat 只能调 Scheduler
max_subagents = 0              # Chat 不唤起 Sub-Agent

# assistant.toml（后台需求分析/草案 Worker）
[role]
name = "assistant"
description = "后台需求分析/草案 Worker"
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

```toml
# evolver.toml
[role]
name = "evolver"
description = "定期扫描白板，提炼模式，生成/优化 Skills 和 MCP"

[[models]]
model = "deepseek/deepseek-chat"
cost_tier = "low"

[permissions]
allowed_tools = ["web_search"]
scope = ["board_read", "board_write"]
max_subagents = 0
can_call_roles = []
can_create_agents = false

skills = ["analysis", "summarization"]

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

skills = []

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

skills = ["task-management"]

[mcp]
servers = []
```
