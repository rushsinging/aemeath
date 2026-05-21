# Multi-Agent 框架 DDD 设计

**日期**：2026-05-20
**修订**：2026-05-21 — 分布式 Server/Agent + Redis 消息层
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
| Watch | MongoDB Change Stream、gRPC streaming、前端订阅语义混用 | 取消 Watch 作为产品/领域词；统一用 `BoardEventStream` / `WorkQueue` / `IntegrationEvent` |
| RPC | 同步远程调用与业务命令混用 | 跨进程协作改为 `Command` + `WorkItem` + `IntegrationEvent`，不以 RPC 作为 Agent 调度边界 |

### 标准词汇表

```text
Workspace          — 租户级隔离单元，包含所有资源
Project            — 用户的一次具体工程目标（有生命周期：Draft → Active → Completed）
Requirement        — 用户意图的结构化表达，由 Assistant 从对话中提炼，需用户确认
Task               — Project 下的业务执行单元，可被独立执行和重试；是 Project 聚合内实体
WorkItem           — 异步执行系统里的工作单元，用于派发给 Agent；不是业务 Task
AgentRun           — Agent 执行某个 WorkItem 的一次尝试，记录 owner、attempt、结果和审计信息
Conversation       — Chat Agent 与用户的一次对话线程（替代“Session”的对话语义）
LlmContext         — Agent 持有的 LLM 消息历史窗口（替代“Session”的 LLM 语义）
MainAgent          — 长期存活、有角色的 Agent（Chat/Scheduler/Executor/Assistant/Evolver）
SubAgent           — 一次性无状态执行单元，接受输入返回输出，不感知白板
AgentRole          — MainAgent 的角色定义（配置驱动：工具白名单、权限、prompt 模板）
AgentInstance      — 一个已启动的 Agent 进程实例，可多副本部署并通过 Redis/MongoDB 协作
ControllerLease    — Scheduler/Evolver 等 controller 在 workspace 维度的独占执行权
DomainEvent        — 聚合产生的业务事实，不包含 Redis stream id，不直接面向 UI
IntegrationEvent   — 跨进程集成契约，由 Outbox 发布到 Redis Streams
BoardEvent         — 面向 UI 的白板投影事件，可由 MongoDB snapshot 重建
OutboxEvent        — MongoDB 中的待发布事件记录，连接状态写入与 Redis 发布
MessageBus         — Redis Streams 承载的集成事件总线
WorkQueue          — Redis Streams 承载的 Agent 工作队列
Presence           — Redis TTL key 表示的 Agent 短期在线状态
Board              — 前端呈现层（UI），通过 REST 获取 snapshot，通过 WebSocket 接收 Redis-backed BoardEvent
API Server         — 无状态 API 与 WebSocket gateway；写 MongoDB、写 Outbox、订阅 Redis 后转发 UI；不直接调度 Agent
```

---

## 2. 问题空间 — 子域划分

### Core Domain（核心域）

> **用户意图到代码执行的完整编排链路**

这是系统唯一真正的核心竞争力，包含三个转换步骤：

```text
用户消息
  → [Chat Agent]    → Requirement（用户意图结构化）
  → [Assistant]     → Project + Task（需求可执行化）
  → [Executor + SubAgent] → 执行结果（任务落地）
```

每一步都有独特的业务规则和状态机，无法用现成方案替代。具体包括：
- 意图捕获：Chat Agent 分析用户消息为结构化 Requirement，引导用户确认
- 需求拆解：Assistant 将 Requirement 拆解为 Project / Task 层级结构
- 任务编排：Scheduler 创建 WorkItem，Executor 消费 WorkQueue 并驱动 SubAgent 执行 Task
- 故障恢复：Agent 崩溃恢复、WorkItem 重试、幂等性保证、Redis pending reclaim、MongoDB 状态对账

### Supporting Subdomain（支撑域）

| 子域 | 说明 |
|---|---|
| **自我进化** | Evolver 扫描历史、提炼 Skill。异步后台，失败不影响核心链路 |
| **平台消息层** | Redis Streams / TTL / consumer group，为 Core Domain 提供跨进程消息、队列、presence 能力，本身不含业务规则 |

### Generic Subdomain（通用域）

| 子域 | 推荐策略 |
|---|---|
| LLM Provider 抽象（packages/llm） | 继续复用，不过度设计 |
| 认证/权限 | 标准中间件，不放业务逻辑 |
| 可观测性（日志、指标、追踪） | 基础设施层，用现成库 |
| 传输协议（REST、WebSocket、Redis Streams） | 纯通信机制，不承载领域规则 |
| MongoDB 持久化 | Repository / Outbox 基础设施，不进入领域模型 |
| Board（前端 UI） | 纯呈现层，消费 API Server snapshot 与 BoardEvent |

---

## 3. 解决方案空间 — Bounded Context 划分

### 六个 Bounded Context

```text
┌─────────────────────────────────────────────────────────────────┐
│                        Core Domain                              │
│                                                                 │
│  ┌──────────────┐    ┌──────────────┐    ┌──────────────────┐  │
│  │ Conversation │    │ Requirement  │    │    Project       │  │
│  │ Context      │───▶│ Context      │───▶│    Context       │  │
│  │              │    │              │    │                  │  │
│  │ Chat Agent   │    │ Assistant    │    │ Project / Task   │  │
│  └──────────────┘    └──────────────┘    └────────┬─────────┘  │
│                                                   │             │
│                                          ┌────────▼──────────┐  │
│                                          │ Orchestration     │  │
│                                          │ Context           │  │
│                                          │ Scheduler         │  │
│                                          │ Executor          │  │
│                                          │ WorkItem / Lease  │  │
│                                          │ SubAgent          │  │
│                                          └───────────────────┘  │
└─────────────────────────────────────────────────────────────────┘

┌──────────────────┐        ┌───────────────────────────────────┐
│ Evolution        │        │ Platform Context                  │
│ Context          │        │ Workspace / Auth / API Server     │
│ Evolver Agent    │        │ MongoDB / Redis / REST / WS       │
└──────────────────┘        └───────────────────────────────────┘
```

### 各 BC 职责边界

**Conversation Context**
- 拥有：`Conversation`、`Message`、`UserIntent`
- 职责：维护用户对话线程，分类用户输入，向用户汇报状态
- 不负责：理解需求的业务含义，只识别“这是一个需求”这个事实

**Requirement Context**
- 拥有：`Requirement`（Pending → Analyzing → Draft → Confirmed / Rejected / Cancelled）
- 职责：将用户意图结构化为可执行的需求草案，等待用户确认
- 边界说明：确认后发出领域事件，Project Context 订阅并负责创建 Project/Task

**Project Context**
- 拥有：`Project`、`Task`（及其状态机）
- 职责：管理 Project/Task 的完整生命周期
- 边界说明：Task 的执行结果由 Orchestration Context 通过应用服务写回，Project Context 只关心状态变更事实

**Orchestration Context**（Core Domain 心脏）
- 拥有：`AgentInstance`、`WorkItem`、`AgentRun`、`ControllerLease`、`ExecutorAssignment`、`SubAgentInvocation`、`RetryPolicy`
- 职责：Scheduler/Evolver 通过 workspace-level `ControllerLease` 多实例运行；Scheduler 产生 WorkItem；Executor/Assistant/Evolver 消费 Redis WorkQueue；Executor 驱动 SubAgent 执行 Task；管理故障恢复、重试、幂等性
- 边界说明：所有 Agent 类型都可多实例部署。Server 不直接调用 Agent，Agent 不依赖 RPC 注册/派发；跨进程协作通过 Redis Streams + MongoDB 状态完成

**Evolution Context**（Supporting Subdomain）
- 拥有：`SkillPattern`、`ProjectSummary`
- 职责：异步扫描已完成 Project，提炼可复用 Skill
- 边界说明：Evolver 可多实例运行，按 workspace-level `ControllerLease` 或 WorkItem shard 避免重复处理

**Platform Context**（Infrastructure）
- 拥有：`Workspace`、`Tenant`、`ApiServer`、`AuthToken`、`MessageBus`、`WorkQueue`、`PresenceStore`、`OutboxPublisher`
- 职责：多租户隔离、MongoDB 持久化、Redis 消息层、REST API、WebSocket gateway、Outbox 发布
- 边界说明：不含业务规则，是其他所有 BC 的基础设施；Redis/MongoDB/WebSocket 不可泄漏进 Domain 层

### Context Map（集成模式）

| 上游 BC | 下游 BC | 集成模式 | 说明 |
|---|---|---|---|
| Conversation | Requirement | **Partnership** | 发布 `UserIntentCaptured` 领域事件；Outbox 转换为 Redis IntegrationEvent |
| Requirement | Project | **Customer/Supplier** | 用户确认后发布 `RequirementConfirmed`，Project BC 消费并创建 Project/Task |
| Project | Orchestration | **Published Language** | 发布 `TaskReadyForExecution`；Orchestration 创建 WorkItem 并投递 Redis WorkQueue |
| Orchestration | Project | **Customer/Supplier（反向写回）** | Agent 执行完成后通过应用服务写回 Task 状态 |
| Orchestration | Platform | **Published Language + Infrastructure Service** | 使用 MessageBus/WorkQueue/PresenceStore 端口，不直接依赖 Redis client |
| Orchestration | LLM（packages/llm） | **Anticorruption Layer** | 隔离 provider 接口变动，内部使用自己的领域语言 |
| Evolution | Project | **Conformist** | 只读已完成 Project 数据，单向依赖 |

### 关于共享存储与消息层的权衡

API Server 与 Agent Runtime 都会访问 MongoDB，但这不意味着可以跨 BC 任意读写。缓解策略：

1. **按 Collection 归属划分 BC 边界**：每个 Repository 只属于一个 BC。
2. **跨 BC 业务流转通过 DomainEvent → OutboxEvent → IntegrationEvent**：NEVER 通过直接改对方 collection 来驱动流程。
3. **Redis 只承载消息与短期 presence**：NEVER 把 Redis Stream 当作领域真相。
4. **API Server 无状态可多副本部署**：任何 server 实例都不能持有唯一调度职责。
5. **Agent Runtime 可多实例部署**：所有抢占、续租、重试、幂等以 MongoDB 状态和 Redis consumer group 为准。

---

## 4. 战术 DDD — 聚合、实体、值对象、领域事件

### 4a. 聚合根与实体

**聚合根判断标准**：它是一个一致性边界——对它内部的任何修改必须整体一致，外部只能通过根来操作内部。

#### Conversation Context

```text
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

```text
Requirement（聚合根）
├── id: RequirementId
├── conversation_id: ConversationId  ← 只保存 id，不引用对象
├── status: RequirementStatus        ← Pending | Analyzing | Draft | InProgress | Completed | Rejected | Cancelled
├── draft: Option<RequirementDraft>  ← 值对象（Assistant 分析结果的不可变快照）
└── confirmed_at: Option<Timestamp>
```

Requirement 不包含 Project，只包含状态。确认后发出 `RequirementConfirmed` 事件，Project Context 订阅并负责创建 Project。**两个聚合之间永远不互相引用对象，只传递 id。**

#### Project Context

```text
Project（聚合根）
├── id: ProjectId
├── requirement_id: RequirementId    ← 溯源引用
├── workspace_id: WorkspaceId
├── status: ProjectStatus            ← Pending | Assigned | InProgress | Blocked | Failed | Completed | Cancelled
├── tasks: Vec<Task>                 ← 实体集合
└── assignment_id: Option<AssignmentId> ← 当前执行分配（只存 id）

Task（Project 内部实体，不是独立聚合根）
├── id: TaskId
├── spec: TaskSpec                   ← 值对象（描述 + 验收标准，不可变）
├── status: TaskStatus               ← Pending | InProgress | InReview | Retrying | Completed | Failed | Cancelled
├── retry_count: u32
└── result: Option<TaskResult>       ← 值对象（执行结果快照）
```

**Task 是 Project 的内部实体，而不是独立聚合根**。原因：“一个 Project 同时只有一个 Active Assignment”这个不变量需要在 Project / ExecutorAssignment 的一致性边界内协同维护。如果 Task 是独立聚合根，这个约束就无法在单次事务内保证。

#### Orchestration Context

```text
AgentInstance（聚合根）
├── id: AgentInstanceId
├── agent_type: AgentType            ← Chat | Scheduler | Executor | Assistant | Evolver
├── role: AgentRole                  ← 值对象（配置快照，不可变）
├── capabilities: Vec<Capability>
├── status: AgentStatus              ← Initializing | Idle | Busy | Draining | Offline | Lost | Error
├── max_concurrency: u32
├── last_heartbeat_at: Timestamp     ← MongoDB 中的可查询摘要
└── presence_key: String             ← Redis TTL key 引用，不是领域真相

WorkItem（聚合根）
├── id: WorkItemId
├── workspace_id: WorkspaceId
├── required_agent_type: AgentType
├── kind: WorkItemKind               ← AnalyzeRequirement | ReconcileWorkspace | ExecuteProject | EvolveSkill | ...
├── payload_ref: PayloadRef          ← 完整 payload 存 MongoDB，Redis 只放引用
├── status: WorkItemStatus           ← Pending | Leased | Running | Succeeded | Failed | Cancelled
├── idempotency_key: IdempotencyKey
├── lease_owner: Option<AgentInstanceId>
├── lease_expires_at: Option<Timestamp>
├── attempt: u32
└── result_ref: Option<ResultRef>

AgentRun（聚合根）
├── id: AgentRunId
├── work_item_id: WorkItemId
├── agent_instance_id: AgentInstanceId
├── attempt: u32
├── status: AgentRunStatus           ← Started | Succeeded | Failed | Cancelled | TimedOut
├── started_at: Timestamp
├── finished_at: Option<Timestamp>
└── audit_refs: Vec<AuditRef>

ControllerLease（聚合根）
├── id: ControllerLeaseId
├── workspace_id: WorkspaceId
├── controller_type: ControllerType  ← Scheduler | Evolver
├── owner_agent_id: AgentInstanceId
├── lease_expires_at: Timestamp
└── generation: u64

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

**`WorkItem` 与 `Task` 必须分离**：Task 是用户可见的业务任务；WorkItem 是执行系统的异步工作单元。Redis WorkQueue 派发 WorkItem，不直接派发 Project Task。
**`ControllerLease` 是分布式 controller 的一致性边界**：Scheduler/Evolver 可以多副本运行，但同一个 `workspace_id + controller_type` 同时最多一个 owner。
**`AgentInstance` 的短期在线状态在 Redis，长期可查询状态在 MongoDB**：Redis TTL 丢失只表示 presence 失效，最终由 reconciler 写回 `Lost` 或 `Offline`。

#### Platform Context

```text
OutboxEvent（聚合根 / 基础设施一致性记录）
├── id: OutboxEventId
├── aggregate_id: String
├── aggregate_type: String
├── domain_event_type: String
├── payload: Json
├── status: OutboxStatus             ← Pending | Published | Failed
├── publish_attempt: u32
├── published_stream: Option<String>
├── published_stream_id: Option<String>
└── created_at: Timestamp
```

OutboxEvent 不是业务领域真相，但它是跨进程最终一致性的强制边界：**MongoDB 聚合状态写入与 OutboxEvent 写入必须同事务或同一原子写路径完成**。

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
| `IdempotencyKey` | 幂等语义的稳定身份，不能复用到不同命令 |
| `PayloadRef` / `ResultRef` | Redis 消息只携带引用，引用不可变 |

**判断标准**：你需要追踪它的历史吗？需要 → Entity；不需要，只关心当前值 → Value Object。

### 4c. 领域事件、集成事件、UI 事件

必须区分三层事件：

1. **DomainEvent**：聚合产生的业务事实，例如“Requirement 已确认”。不包含 Redis stream id，不直接面向 UI。
2. **IntegrationEvent**：跨进程通知契约，由应用层 / OutboxPublisher 从 DomainEvent 转换并发布到 Redis Streams。
3. **BoardEvent**：面向 UI 的白板投影事件，可从 MongoDB snapshot 重建，经 WebSocket gateway 推送给前端。

```text
Conversation Context 发布 DomainEvent：
  UserIntentCaptured        { conversation_id, intent, captured_at }
  MessageAppended           { conversation_id, message_id, message_type, appended_at }

Requirement Context 发布 DomainEvent：
  RequirementDraftCreated   { requirement_id, conversation_id, draft }
  RequirementConfirmed      { requirement_id, workspace_id, confirmed_at }
  RequirementRejected       { requirement_id, reason }

Project Context 发布 DomainEvent：
  ProjectCreated            { project_id, requirement_id, workspace_id }
  TaskReadyForExecution     { project_id, task_id, spec }
  TaskStatusChanged         { project_id, task_id, old_status, new_status }
  ProjectCompleted          { project_id, completed_at }

Orchestration Context 发布 DomainEvent：
  WorkItemRequested         { work_item_id, required_agent_type, kind }
  WorkItemLeased            { work_item_id, agent_instance_id, lease_expires_at }
  WorkItemCompleted         { work_item_id, result_ref }
  ProjectAssignedToExecutor { project_id, executor_id, assignment_id }
  SubAgentInvocationCompleted { assignment_id, task_id, result }
  AgentLost                 { agent_instance_id, last_heartbeat_at }
  AssignmentReleased        { assignment_id, project_id, reason }
```

事件转换规则：

```text
DomainEvent
  → OutboxEvent（MongoDB）
  → IntegrationEvent（Redis Streams）
  → BoardEvent / WorkQueue message / Control signal（按用途投递）
```

NEVER 用 Redis Stream 消息替代 DomainEvent。Redis message 是传输契约，可以版本化、裁剪字段、只携带引用；DomainEvent 是业务事实。

### 4d. 领域服务、应用服务与仓储

领域服务用于跨聚合的业务逻辑，不属于任何单一聚合：

| 服务 | 层 | BC | 职责 |
|---|---|---|---|
| `AssignmentService` | Domain Service | Orchestration | 决策哪个 Executor 分配给哪个 Project（路由算法不属于任何聚合） |
| `RetryPolicy` | Domain Service | Orchestration | 判断 Task / WorkItem 是否可重试、等待策略 |
| `RequirementAnalysisService` | Domain Service | Requirement | 驱动 Assistant LLM 调用，将 UserIntent → RequirementDraft |
| `AppendMessageUseCase` | Application Service | Conversation | 写 Conversation/Message，产生 OutboxEvent |
| `RequestWorkItemUseCase` | Application Service | Orchestration | 创建 WorkItem，写 Outbox，等待 publisher 投递 WorkQueue |
| `CompleteWorkItemUseCase` | Application Service | Orchestration | 校验 owner/attempt，写 AgentRun/WorkItem 结果，产生后续事件 |
| `PublishOutboxUseCase` | Application Service | Platform | 读取 MongoDB outbox_events，发布到 Redis Streams，回写 stream id |
| `ConsumeWorkQueueUseCase` | Application Service | Agent Runtime | XREADGROUP 拉取消息，加载 WorkItem，执行业务用例，XACK |

仓储（Repository）对应关系：

```text
WorkspaceRepository       → MongoDB workspaces collection（Platform Context 独占）
ConversationRepository    → MongoDB conversations / chat_messages collection（Conversation Context 独占）
RequirementRepository     → MongoDB requirements collection（Requirement Context 独占）
ProjectRepository         → MongoDB projects collection（Project Context 独占）
AgentInstanceRepository   → MongoDB agent_instances collection（Orchestration Context 独占）
WorkItemRepository        → MongoDB work_items collection（Orchestration Context 独占）
AgentRunRepository        → MongoDB agent_runs collection（Orchestration Context 独占）
ControllerLeaseRepository → MongoDB controller_leases collection（Orchestration Context 独占）
AssignmentRepository      → MongoDB executor_assignments collection（Orchestration Context 独占）
OutboxRepository          → MongoDB outbox_events collection（Platform Context 独占）
```

每个 Repository 只属于一个 BC，禁止跨 BC 共享 Repository。

Redis 封装为基础设施端口，而不是 Repository：

```text
MessageBus       → Redis Streams integration event stream
WorkQueue        → Redis Streams consumer group work queue
BoardEventStream → Redis Streams board event stream
PresenceStore    → Redis TTL keys for short-lived agent presence
ControlSignalBus → Redis Streams scheduler/evolver/cancel signal stream
```

Domain layer MUST NOT depend on Redis, MongoDB, WebSocket, or concrete SDK clients.

---

## 5. 分布式一致性与消息边界

### 5a. MongoDB 与 Redis 职责

| 组件 | 职责 | 不负责 |
|---|---|---|
| MongoDB | 聚合状态、Outbox、幂等记录、AgentRun 审计、Board snapshot projection | 实时消息分发 |
| Redis Streams | IntegrationEvent、WorkQueue、BoardEvent、control signal、consumer group ack/reclaim | 领域真相、长期审计 |
| Redis TTL keys | Agent presence、短期心跳、轻量锁提示 | 长期 Agent 状态 |
| API Server | REST command/query、WebSocket gateway、Outbox publisher 可选承载 | 唯一调度、直接调用 Agent |
| Agent Runtime | Redis worker、LLM/tool 执行、应用服务调用 | 直接绕过用例改数据库 |

### 5b. Outbox 是强制一致性边界

所有会触发跨进程协作的写操作 MUST 使用 Outbox：

```text
1. Application Service 加载聚合
2. 调用聚合方法产生 DomainEvent
3. Repository 保存聚合状态
4. OutboxRepository 保存 OutboxEvent
5. 返回 command accepted / resource state
6. OutboxPublisher 异步发布 Redis IntegrationEvent
7. 发布成功后记录 stream name / stream id
```

这样避免：MongoDB 写成功但 Redis 发布失败，导致 UI 或 Agent 永远收不到事件。

### 5c. Agent 工作队列

Redis WorkQueue 只放路由与引用字段：

```text
stream: aemeath:work:{agent_type}
group: agents:{agent_type}
consumer: {agent_instance_id}
message:
  work_item_id
  workspace_id
  required_agent_type
  kind
  idempotency_key
  payload_ref
  created_at
```

Agent 消费流程：

```text
1. XREADGROUP 读取 WorkQueue
2. 根据 work_item_id 加载 WorkItem 聚合
3. MongoDB 原子 claim / start，校验 lease_owner、attempt、status
4. 执行 LLM / tool / SubAgent
5. 通过 Application Service 写结果和 DomainEvent
6. XACK Redis message
7. 失败时记录 AgentRun，按 RetryPolicy 决定重试或失败
```

Redis pending 只是传输层未 ack 状态；WorkItem 的真实状态仍以 MongoDB 为准。XAUTOCLAIM 后必须重新校验 MongoDB lease，NEVER 仅凭 Redis pending 判断可执行。

### 5d. Server / Agent 完全分布式

- API Server MAY 多副本部署，副本之间不共享内存状态。
- Agent Runtime MAY 按任意 agent_type 多实例部署。
- Scheduler / Evolver MUST 使用 `ControllerLease` 或 WorkItem shard 避免重复 reconcile。
- Executor / Assistant SHOULD 通过 WorkQueue + lease 抢占任务。
- Server MUST NOT 点名调用某个 Agent。
- Agent MUST NOT 依赖 RPC 注册/心跳接口；注册状态写 MongoDB，短期 presence 写 Redis TTL key。

### 5e. UI 实时更新

Board UI 数据分为两类：

1. REST query 获取当前 Board snapshot。
2. WebSocket gateway 推送 Redis-backed BoardEvent。

WebSocket 是 UI 传输协议，不是领域事件机制。客户端重连时携带 `last_stream_id`，server 从 Redis BoardEventStream 尝试补发；若 stream 已裁剪或 gap 不可恢复，则返回 `snapshot_required`，客户端重新 REST 拉取 snapshot。

---

## 6. 关键设计张力总结

| 张力 | 旧设计 | DDD 视角 | 新决策 |
|---|---|---|---|
| API Server 共享存储 | 所有 BC 共用 MongoDB | 接近集成数据库反模式 | 按 Collection 归属划分逻辑边界，跨 BC 通过 DomainEvent/Outbox/IntegrationEvent |
| Task 是否独立聚合 | Task 作为 Project 子对象 | Task 独立则无法原子保证 Project 执行不变量 | 保持 Task 为 Project 内部实体；新增 WorkItem 作为执行队列聚合 |
| gRPC Watch vs 领域事件 | Watch 驱动 Agent 响应 | Watch 是传输机制，不是业务语义 | 取消 Watch；DomainEvent 经 Outbox 转 Redis IntegrationEvent |
| RPC Agent 调度 | Server/Scheduler 直接调用或 Watch Agent | 同步调用隐藏分布式失败边界 | 取消 Agent RPC 调度；改为 Redis WorkQueue + MongoDB lease |
| Redis 是否进入领域 | 可能把 Stream 当事件源 | Redis 是基础设施传输，不是领域真相 | Domain 不依赖 Redis；Redis message 是 IntegrationEvent / WorkQueue contract |
| Scheduler 职责过重 | 单例 Scheduler 管池与路由 | 单例无法分布式部署 | Scheduler 多实例；workspace-level ControllerLease；AssignmentService 只保留路由决策 |
| UI 事件与领域事件混用 | BoardSnapshotUpdate 既像领域事件又像传输 payload | UI 投影不是领域事实 | BoardEvent 是 UI projection，可从 MongoDB snapshot 重建 |
