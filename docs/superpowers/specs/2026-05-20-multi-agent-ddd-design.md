# Multi-Agent 框架 DDD 设计

**日期**：2026-05-20
**方法**：Strategic DDD → Tactical DDD（自顶向下）
**范围**：aemeath Multi-Agent 框架（Feature #36）

---

## 1. Ubiquitous Language（通用语言）

DDD 的基础是对齐语言。现有设计存在以下命名冲突，需要统一：

| 旧词 | 问题 | 统一后 |
|---|---|---|
| Session | LLM 上下文 / 用户登录会话两种含义 | 拆为 `Conversation`（对话线程）和 `LlmContext`（LLM 消息历史窗口） |
| Agent | 长期存活的 Main Agent / 一次性 Sub-Agent 混用 | 拆为 `MainAgent` 和 `SubAgent` 作为一等公民词汇 |
| Task | 领域执行单元 / TUI 任务列表条目混用 | 领域概念保留 `Task`，TUI 层改为 `UiTask` |
| Context | Agent 上下文 / 应用运行时上下文重载 | 领域相关用 `LlmContext`，运行时用 `AppContext` |

### 标准词汇表

```
Workspace       — 租户级隔离单元，包含所有资源
Project         — 用户的一次具体工程目标（有生命周期：Draft → Active → Completed）
Requirement     — 用户意图的结构化表达，由 Assistant 从对话中提炼，需用户确认
Task            — Project 下的最小执行单元，可被独立执行和重试（仅此含义）
Conversation    — Chat Agent 与用户的一次对话线程（替代"Session"的对话语义）
LlmContext      — Agent 持有的 LLM 消息历史窗口（替代"Session"的 LLM 语义）
MainAgent       — 长期存活、有角色的 Agent（Chat/Scheduler/Executor/Assistant/Evolver）
SubAgent        — 一次性无状态执行单元，接受输入返回输出，不感知白板
AgentRole       — MainAgent 的角色定义（配置驱动：工具白名单、权限、prompt 模板）
AgentPool       — Scheduler 管理的 MainAgent 实例集合，按 role 分组
Board           — 前端呈现层（UI），通过 REST/WebSocket 读取 API Server 数据渲染
API Server      — 数据读写层，所有 BC 通过它访问 MongoDB，提供 gRPC + REST + WebSocket 接口
```

---

## 2. 问题空间 — 子域划分

### Core Domain（核心域）

> **用户意图到代码执行的完整编排链路**

这是系统唯一真正的核心竞争力，包含三个转换步骤：

```
用户消息
  → [Chat Agent]    → Requirement（用户意图结构化）
  → [Assistant]     → Project + Task（需求可执行化）
  → [Executor + SubAgent] → 执行结果（任务落地）
```

每一步都有独特的业务规则和状态机，无法用现成方案替代。具体包括：
- 意图捕获：Chat Agent 分析用户消息为结构化 Requirement，引导用户确认
- 需求拆解：Assistant 将 Requirement 拆解为 Project / Task 层级结构
- 任务编排：Scheduler 分配 Executor，Executor 驱动 SubAgent 执行 Task
- 故障恢复：崩溃恢复、Task 重试、幂等性保证

### Supporting Subdomain（支撑域）

| 子域 | 说明 |
|---|---|
| **自我进化** | Evolver 扫描历史、提炼 Skill。异步后台，失败不影响核心链路 |
| **API Server 数据层** | 状态持久化与分发，为 Core Domain 提供一致性保证，本身不含编排逻辑 |

### Generic Subdomain（通用域）

| 子域 | 推荐策略 |
|---|---|
| LLM Provider 抽象（packages/llm） | 继续复用，不过度设计 |
| 认证/权限 | 标准中间件，不放业务逻辑 |
| 可观测性（日志、指标、追踪） | 基础设施层，用现成库 |
| 传输协议（gRPC、REST、WebSocket） | 纯通信机制，不承载领域规则 |
| Board（前端 UI） | 纯呈现层，消费 API Server 数据 |

---

## 3. 解决方案空间 — Bounded Context 划分

### 六个 Bounded Context

```
┌─────────────────────────────────────────────────────────────────┐
│                        Core Domain                              │
│                                                                 │
│  ┌──────────────┐    ┌──────────────┐    ┌──────────────────┐  │
│  │  Conversation│    │  Requirement │    │    Project        │  │
│  │  Context     │───▶│  Context     │───▶│    Context        │  │
│  │              │    │              │    │                   │  │
│  │  Chat Agent  │    │  Assistant   │    │  Project / Task   │  │
│  └──────────────┘    └──────────────┘    └────────┬──────────┘  │
│                                                   │             │
│                                          ┌────────▼──────────┐  │
│                                          │  Orchestration    │  │
│                                          │  Context          │  │
│                                          │                   │  │
│                                          │  Scheduler        │  │
│                                          │  Executor         │  │
│                                          │  SubAgent         │  │
│                                          └───────────────────┘  │
└─────────────────────────────────────────────────────────────────┘

┌──────────────────┐        ┌───────────────────────────────────┐
│  Evolution       │        │  Platform Context                 │
│  Context         │        │                                   │
│  (Supporting)    │        │  Workspace / Auth / API Server    │
│  Evolver Agent   │        │  MongoDB / gRPC / WebSocket       │
└──────────────────┘        └───────────────────────────────────┘
```

### 各 BC 职责边界

**Conversation Context**
- 拥有：`Conversation`、`Message`、`UserIntent`
- 职责：维护用户对话线程，分类用户输入，向用户汇报状态
- 不负责：理解需求的业务含义，只识别"这是一个需求"这个事实

**Requirement Context**
- 拥有：`Requirement`（Draft → PendingConfirmation → Confirmed / Rejected）
- 职责：将用户意图结构化为可执行的需求草案，等待用户确认
- 边界说明：确认后发出事件，Project Context 订阅并负责创建 Project/Task

**Project Context**
- 拥有：`Project`、`Task`（及其状态机）
- 职责：管理 Project/Task 的完整生命周期
- 边界说明：Task 的执行结果由 Orchestration Context 写回，Project Context 只关心状态变更事实

**Orchestration Context**（Core Domain 心脏）
- 拥有：`AgentPool`、`ExecutorAssignment`、`SubAgentInvocation`、`RetryPolicy`
- 职责：Scheduler 调度 Executor，Executor 驱动 SubAgent 执行 Task；管理故障恢复、重试、幂等性
- 边界说明：业务规则最密集，崩溃恢复逻辑、Task 重试状态机都在这里

**Evolution Context**（Supporting Subdomain）
- 拥有：`SkillPattern`、`ProjectSummary`
- 职责：异步扫描已完成 Project，提炼可复用 Skill
- 边界说明：单向读取 Project Context 历史数据，失败不影响主链路

**Platform Context**（Infrastructure）
- 拥有：`Workspace`、`Tenant`、`ApiServer`、`AuthToken`
- 职责：多租户隔离、数据持久化（MongoDB）、gRPC/REST/WebSocket 传输、Board UI 数据服务
- 边界说明：不含业务规则，是其他所有 BC 的基础设施

### Context Map（集成模式）

| 上游 BC | 下游 BC | 集成模式 | 说明 |
|---|---|---|---|
| Conversation | Requirement | **Partnership** | 发布 `UserIntentCaptured` 事件，两者需要紧密协作共同进化 |
| Requirement | Project | **Customer/Supplier** | 用户确认后发布 `RequirementConfirmed`，Project BC 消费并创建 Project/Task |
| Project | Orchestration | **Published Language** | 发布 `TaskReadyForExecution` 等标准事件，双方通过事件契约解耦 |
| Orchestration | Project | **Customer/Supplier（反向写回）** | 完成执行后写回 Task 状态，Project BC 接受结果 |
| Orchestration | Platform | **Conformist** | 直接使用 API Server 接口，跟随其 API 设计 |
| Orchestration | LLM（packages/llm） | **Anticorruption Layer** | 隔离 provider 接口变动，内部使用自己的领域语言 |
| Evolution | Project | **Conformist** | 只读已完成 Project 数据，单向依赖 |

### 关于 API Server 共享的权衡

API Server 是所有 BC 共用的数据层，接近 DDD 中的**集成数据库（Integration Database）反模式**。

缓解策略：**按 Collection 归属划分 BC 边界**，每个 BC 只通过自己的 gRPC API 访问自己的 Collection，禁止跨 BC 直接读写他人 Collection。这是在工程成本和 DDD 纯度之间的合理权衡。

---

## 4. 战术 DDD — 聚合、实体、值对象、领域事件

### 4a. 聚合根与实体

**聚合根判断标准**：它是一个一致性边界——对它内部的任何修改必须整体一致，外部只能通过根来操作内部。

#### Conversation Context

```
Conversation（聚合根）
├── id: ConversationId
├── workspace_id: WorkspaceId
├── status: ConversationStatus       ← 值对象
├── messages: Vec<Message>           ← 实体（有顺序、有 id）
└── current_intent: Option<UserIntent>  ← 值对象（分析快照，不可变）
```

`Message` 是实体：有身份，可被引用、可被追溯，顺序和 id 有业务意义。
`UserIntent` 是值对象：某次分析的不可变结果，改了就是新的 Intent。

#### Requirement Context

```
Requirement（聚合根）
├── id: RequirementId
├── conversation_id: ConversationId  ← 只保存 id，不引用对象
├── status: RequirementStatus        ← Draft | AnalysisPending | PendingConfirmation | Confirmed | Rejected
├── draft: Option<RequirementDraft>  ← 值对象（Assistant 分析结果的不可变快照）
└── confirmed_at: Option<Timestamp>
```

Requirement 不包含 Project，只包含状态。确认后发出 `RequirementConfirmed` 事件，Project Context 订阅并负责创建 Project。**两个聚合之间永远不互相引用对象，只传递 id。**

#### Project Context

```
Project（聚合根）
├── id: ProjectId
├── requirement_id: RequirementId    ← 溯源引用
├── workspace_id: WorkspaceId
├── status: ProjectStatus            ← Draft | Active | Paused | Completed | Archived
├── tasks: Vec<Task>                 ← 实体集合
└── executor_id: Option<ExecutorId>  ← 当前分配的 Executor（只存 id）

Task（Project 内部实体，不是独立聚合根）
├── id: TaskId
├── spec: TaskSpec                   ← 值对象（描述 + 验收标准，不可变）
├── status: TaskStatus               ← Pending | InProgress | InReview | Retrying | Completed | Failed
├── retry_count: u32
└── result: Option<TaskResult>       ← 值对象（执行结果快照）
```

**Task 是 Project 的内部实体，而不是独立聚合根**。原因："一个 Project 只分配给一个 Executor"这个不变量需要在 Project 聚合内部维护。如果 Task 是独立聚合根，这个约束就无法在单次事务内保证。

#### Orchestration Context

```
AgentInstance（聚合根）
├── id: AgentInstanceId
├── role: AgentRole                  ← 值对象（配置快照，不可变）
├── status: AgentStatus              ← Idle | Busy | Crashed | Terminated
├── assigned_project: Option<ProjectId>
└── last_heartbeat: Timestamp

ExecutorAssignment（聚合根）
├── id: AssignmentId
├── project_id: ProjectId
├── executor_id: AgentInstanceId
├── assigned_at: Timestamp
├── status: AssignmentStatus         ← Active | Released | Crashed
└── invocations: Vec<SubAgentInvocation>  ← 实体

SubAgentInvocation（ExecutorAssignment 内部实体）
├── id: InvocationId
├── task_id: TaskId
├── role: AgentRole                  ← 值对象
├── input: InvocationInput           ← 值对象
├── output: Option<InvocationOutput> ← 值对象
└── retry_count: u32
```

**`ExecutorAssignment` 作为独立聚合根**：分配关系是一个独立的一致性边界——"一个 Project 同时只有一个 Active Assignment"这个不变量需要在此聚合内维护，与 Agent 实例的生命周期和 Project 的任务结构解耦。

### 4b. 值对象原则

**值对象本质**：没有身份，靠属性相等判断，创建后不可变。

| 值对象 | 不可变原因 |
|---|---|
| `UserIntent` | 一次分析的结论，改了就是新的分析 |
| `RequirementDraft` | Assistant 产出的草案快照，不在原地修改 |
| `TaskSpec` | 任务描述一旦确认不应被修改，修改意味着新 Task |
| `AgentRole` | 角色配置是一次装配的结果，运行时不变 |
| `InvocationInput/Output` | 调用的入参/出参是不可变记录 |
| `TaskResult` | 执行结果快照 |

**判断标准**：你需要追踪它的历史吗？需要 → Entity；不需要，只关心当前值 → Value Object。

### 4c. 领域事件

领域事件是**业务语义层**（"需求被确认了"），不同于 gRPC Watch 的**传输机制层**（"这条记录变了"）。领域事件驱动跨 BC 的业务流转，gRPC Watch 是 API Server 通知消费方数据变化的实现手段。不应用 Watch 替代领域事件。

```
Conversation Context 发布：
  UserIntentCaptured        { conversation_id, intent, captured_at }

Requirement Context 发布：
  RequirementDraftCreated   { requirement_id, conversation_id, draft }
  RequirementConfirmed      { requirement_id, workspace_id, confirmed_at }
  RequirementRejected       { requirement_id, reason }

Project Context 发布：
  ProjectCreated            { project_id, requirement_id, workspace_id }
  TaskReadyForExecution     { project_id, task_id, spec }
  TaskStatusChanged         { project_id, task_id, old_status, new_status }
  ProjectCompleted          { project_id, completed_at }

Orchestration Context 发布：
  ProjectAssignedToExecutor       { project_id, executor_id, assignment_id }
  SubAgentInvocationCompleted     { assignment_id, task_id, result }
  ExecutorCrashed                 { executor_id, project_id, crashed_at }
  AssignmentReleased              { assignment_id, project_id, reason }
```

### 4d. 领域服务与仓储

领域服务用于跨聚合的业务逻辑，不属于任何单一聚合：

| 领域服务 | BC | 职责 |
|---|---|---|
| `AssignmentService` | Orchestration | 决策哪个 Executor 分配给哪个 Project（路由算法不属于任何聚合） |
| `RetryPolicy` | Orchestration | 判断 Task 是否可重试、等待策略（横跨 Assignment 和 Task） |
| `RequirementAnalysisService` | Requirement | 驱动 Assistant LLM 调用，将 UserIntent → RequirementDraft |

仓储（Repository）对应关系：

```
ProjectRepository       → MongoDB projects collection（Project Context 独占）
AgentInstanceRepository → MongoDB agent_instances collection（Orchestration Context 独占）
AssignmentRepository    → MongoDB executor_assignments collection（Orchestration Context 独占）
RequirementRepository   → MongoDB requirements collection（Requirement Context 独占）
ConversationRepository  → MongoDB conversations collection（Conversation Context 独占）
```

每个 Repository 只属于一个 BC，禁止跨 BC 共享 Repository。

---

## 5. 关键设计张力总结

| 张力 | 现有设计 | DDD 视角 | 建议 |
|---|---|---|---|
| API Server 共享存储 | 所有 BC 共用 MongoDB | 接近集成数据库反模式 | 按 Collection 归属划分逻辑边界，禁止跨 BC 直接读写 |
| Task 是否独立聚合 | Task 作为 Project 子对象 | Task 独立则无法原子保证"一 Project 一 Executor" | 保持 Task 为 Project 内部实体 ✓ |
| gRPC Watch vs 领域事件 | Watch 驱动 Agent 响应 | Watch 是传输机制，不是业务语义 | 补充显式领域事件层，Watch 作为事件的传输载体 |
| Scheduler 职责过重 | 池管理 + 路由决策合一 | 两类关注点变化原因不同 | 内部拆分为 `PoolManager`（生命周期）和 `AssignmentService`（路由），暂不拆成两个 BC |
