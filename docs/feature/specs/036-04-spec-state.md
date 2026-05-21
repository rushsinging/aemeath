# #36 多 Agent 框架 — Spec / 状态机设计

> **DDD 设计参考**：[Multi-Agent 框架 DDD 设计](../../superpowers/specs/2026-05-20-multi-agent-ddd-design.md) — ProjectTask 是 Project 聚合的子实体；WorkItem、AgentRun、ControllerLease、ExecutorAssignment 属于 Orchestration Context；OutboxEvent 是 MongoDB 状态写入与 Redis 发布之间的一致性边界。

状态枚举 Rust 侧使用 PascalCase，序列化到 MongoDB 使用 snake_case（通过 `#[serde(rename_all = "snake_case")]`）。

## 核心状态枚举

### ConversationStatus

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConversationStatus {
    Active,
    Archived,
}
```

### RequirementStatus

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RequirementStatus {
    Pending,        // 待分析；Scheduler 会创建 AnalyzeRequirement WorkItem
    Analyzing,      // Assistant WorkItem 正在执行
    Draft,          // 草案已产出，等待用户确认
    InProgress,     // 关联 Project/Task 正在执行中
    Completed,      // 所有关联 Project 终态且成功完成
    Rejected,       // 用户驳回草案
    Cancelled,      // 用户取消
}
```

### ProjectStatus

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ProjectStatus {
    Pending,        // 等待 Scheduler 创建执行类 WorkItem
    Assigned,       // 已有活跃 ExecutorAssignment / WorkItem lease，等待执行开始
    InProgress,     // Executor 已开始执行
    Blocked,        // 等待用户反馈
    Failed,         // 执行失败终态之一；需人工重开
    Completed,      // 全部 ProjectTask 完成
    Cancelled,      // 用户或系统取消
}
```

### ProjectTaskStatus

> **DDD 语义**：ProjectTask 是 Project 聚合的子实体，所有状态变更通过 Project Context 应用服务完成。执行系统派发的是 WorkItem，不是直接派发 ProjectTask。

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ProjectTaskStatus {
    Pending,
    InProgress,
    InReview,
    Completed,
    Failed,
    Retrying,
    Cancelled,
}
```

### AgentStatus

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AgentStatus {
    Initializing,   // Agent runtime 启动并写入 AgentInstance
    Idle,           // 在线且未达到 max_concurrency
    Busy,           // 正在执行一个或多个 WorkItem
    Draining,       // 停止接收新 WorkItem，等待当前执行结束或释放 lease
    Offline,        // 正常下线
    Lost,           // Redis presence / MongoDB heartbeat 超时
    Error,          // 配置、依赖或执行环境异常
}
```

说明：Redis presence TTL 是短期在线信号；MongoDB `agent_instances.status` 是可查询摘要。两者不一致时，以应用服务对账结果为准。

### WorkItemStatus

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WorkItemStatus {
    Pending,        // 已创建，等待 Outbox 发布或等待 worker 消费
    Leased,         // 某个 AgentInstance 已 claim，但尚未开始业务执行
    Running,        // AgentRun 已开始
    Succeeded,      // 执行成功
    Failed,         // 重试耗尽或不可恢复失败
    Cancelled,      // 用户或系统取消
}
```

### AgentRunStatus

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AgentRunStatus {
    Started,
    Succeeded,
    Failed,
    Cancelled,
    TimedOut,
}
```

### ControllerLeaseStatus

ControllerLease 通常不需要独立 status 字段；有效性由 `lease_expires_at > now` 判断。需要归档审计时 MAY 增加：

```rust
pub enum ControllerLeaseStatus {
    Active,
    Expired,
    Released,
}
```

### OutboxStatus

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OutboxStatus {
    Pending,
    Publishing,
    Published,
    Failed,
}
```

### AssignmentStatus

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AssignmentStatus {
    Active,
    Released,
    Crashed,
}
```

不变量：同一 Project 任意时刻最多一个 Active ExecutorAssignment。

## 状态流转

### Requirement 状态流转

```text
Pending ──(Scheduler 创建 AnalyzeRequirement WorkItem)──▶ Pending
Pending ──(Assistant claim WorkItem)────────────────────▶ Analyzing
Pending ──(用户取消)───────────────────────────────────▶ Cancelled

Analyzing ──(Assistant 产出草案)────────────────────────▶ Draft
Analyzing ──(WorkItem lease 超时 / Agent Lost)──────────▶ Pending
Analyzing ──(分析失败但可重试)──────────────────────────▶ Pending
Analyzing ──(重试耗尽 / 不可恢复失败)──────────────────▶ Rejected
Analyzing ──(用户取消)─────────────────────────────────▶ Cancelled

Draft ──(用户确认；事务性创建 Project/Task)────────────▶ InProgress
Draft ──(用户驳回并要求重生成)────────────────────────▶ Pending
Draft ──(用户驳回并放弃)──────────────────────────────▶ Rejected
Draft ──(用户取消)────────────────────────────────────▶ Cancelled

Rejected ──(用户重新提交)─────────────────────────────▶ Pending
InProgress ──(所有关联 Project 完成)──────────────────▶ Completed
InProgress ──(用户取消；级联取消非终态 Project)────────▶ Cancelled
```

### Project 状态流转

```text
Pending ──(Scheduler 创建 ExecuteProject/ExecuteTask WorkItem)──▶ Pending
Pending ──(Executor claim WorkItem 并创建 Active Assignment)────▶ Assigned
Pending ──(用户取消 / Requirement 级联取消)────────────────────▶ Cancelled

Assigned ──(Executor 开始执行)────────────────────────────────▶ InProgress
Assigned ──(WorkItem lease 超时 / Agent Lost)─────────────────▶ Pending
Assigned ──(用户取消；释放 Assignment/WorkItem)───────────────▶ Cancelled

InProgress ──(全部 Task 完成)────────────────────────────────▶ Completed
InProgress ──(Executor 报告需要用户反馈)─────────────────────▶ Blocked
InProgress ──(执行失败且不可自动重试)───────────────────────▶ Failed
InProgress ──(WorkItem lease 超时 / Agent Lost)──────────────▶ Pending
InProgress ──(用户取消；Cooperative Cancel)──────────────────▶ Cancelled

Blocked ──(用户反馈已写入 ConversationMessage)──────────────▶ InProgress
Blocked ──(blocked_timeout_sec 超时)────────────────────────▶ Failed
Blocked ──(用户取消)────────────────────────────────────────▶ Cancelled

Failed ──(人工重开)─────────────────────────────────────────▶ Pending
Cancelled ──(人工重开，仅用户显式操作)──────────────────────▶ Pending
```

级联规则：

- Project 进入 `Cancelled` 时，所有非终态 Task（Pending / InProgress / InReview / Retrying）级联为 Cancelled。
- Project 进入 `Failed` 时，非终态且非 Pending 的 Task 级联为 Failed；Pending Task 保持 Pending，但因 Project 终态不会继续调度。
- 崩溃恢复回到 Pending 时，仅非终态执行中 Task（InProgress / InReview / Retrying）回退 Pending，并保留 retry_count / last_error。

### ProjectTask 状态流转

```text
Pending ──(Executor 开始执行)──────────────────────────▶ InProgress
InProgress ──(需要 review)────────────────────────────▶ InReview
InProgress ──(执行成功且无需 review)──────────────────▶ Completed
InProgress ──(可重试失败)────────────────────────────▶ Retrying
InProgress ──(不可恢复失败)──────────────────────────▶ Failed

InReview ──(review 通过)─────────────────────────────▶ Completed
InReview ──(review 要求返工)─────────────────────────▶ InProgress
InReview ──(review 判定可重试)───────────────────────▶ Retrying
InReview ──(review 判定失败)─────────────────────────▶ Failed

Retrying ──(冷却后重新排队)──────────────────────────▶ Pending
Retrying ──(重试耗尽)────────────────────────────────▶ Failed

Failed ──(人工重开)──────────────────────────────────▶ Pending
Cancelled ──(人工重开；所属 Project 非 Cancelled)────▶ Pending
Pending / InProgress / InReview / Retrying ──(用户取消)──▶ Cancelled
```

## WorkItem 生命周期

### 状态流转

```text
Pending ──(OutboxPublisher 投递 Redis WorkQueue)────────▶ Pending
Pending ──(Agent XREADGROUP + MongoDB claim 成功)──────▶ Leased
Leased ──(AgentRun 创建并开始执行)──────────────────────▶ Running
Leased ──(lease_expires_at 超时)───────────────────────▶ Pending
Running ──(执行成功)───────────────────────────────────▶ Succeeded
Running ──(可重试失败)─────────────────────────────────▶ Pending
Running ──(重试耗尽 / 不可恢复失败)───────────────────▶ Failed
Running ──(用户取消 / control signal)──────────────────▶ Cancelled
Running ──(lease_expires_at 超时 / Agent Lost)────────▶ Pending
```

### Redis 与 MongoDB 对账规则

- Redis message pending 表示传输层尚未 XACK，不等于 WorkItem 可执行。
- Agent 使用 `XAUTOCLAIM` 接管 pending message 后，MUST 重新加载 MongoDB WorkItem 并校验 status、lease_owner、lease_expires_at、attempt。
- WorkItem 的 claim/start/complete MUST 是 MongoDB 原子条件更新。
- WorkItem 成功或终态失败后，Agent MUST XACK 对应 Redis message。
- 如果 Redis message 丢失但 MongoDB WorkItem 仍 Pending，Scheduler/reconciler MUST 能重新写 Outbox 或直接补投递 WorkQueue。

### WorkItem claim 条件

```text
可 claim 条件：
- status in [Pending, Leased, Running]
- required_agent_type == current agent type
- status == Pending OR lease_expires_at < now
- attempt < max_attempts
- cancel_requested_at is null
```

claim 成功后设置：

```text
status = Leased
lease_owner = agent_instance_id
lease_expires_at = now + work_item_lease_ttl
attempt = attempt + 1
```

开始执行时：

```text
status = Running
agent_run_id = new AgentRun
started_at = now
```

## AgentInstance 生命周期

```text
Initializing ──(依赖初始化成功 + 注册 MongoDB 摘要 + Redis presence)──▶ Idle
Initializing ──(配置/依赖失败)──────────────────────────────────────▶ Error
Idle ──(claim WorkItem)────────────────────────────────────────────▶ Busy
Idle ──(收到 drain signal)────────────────────────────────────────▶ Draining
Idle ──(正常停止)────────────────────────────────────────────────▶ Offline
Idle ──(presence/heartbeat 超时)──────────────────────────────────▶ Lost

Busy ──(所有 WorkItem 完成)───────────────────────────────────────▶ Idle
Busy ──(收到 drain signal)────────────────────────────────────────▶ Draining
Busy ──(presence/heartbeat 超时)──────────────────────────────────▶ Lost
Busy ──(执行环境异常)─────────────────────────────────────────────▶ Error

Draining ──(当前 WorkItem 完成或释放 lease)───────────────────────▶ Offline
Draining ──(drain_timeout_sec 超时)───────────────────────────────▶ Lost

Lost ──(Agent 恢复并确认 lease 未被接管)──────────────────────────▶ Idle
Lost ──(reconciler 已释放全部 lease)──────────────────────────────▶ Offline
Error ──(可恢复错误冷却后恢复)────────────────────────────────────▶ Idle
Error ──(不可恢复错误)────────────────────────────────────────────▶ Offline
```

Agent 恢复竞态处理：

1. 恢复后先刷新 Redis presence 与 MongoDB heartbeat。
2. 查询自己持有的 WorkItem lease。
3. 若 lease 已过期或 owner 变更，MUST 停止本地执行并清理临时资源。
4. 若 AgentRun 已被其他实例接管，MUST 不再写回结果。

## ControllerLease 生命周期

ControllerLease 用于 Scheduler/Evolver 多实例部署。

```text
Expired / Missing ──(Agent 原子抢占)──▶ Active(owner=A, generation+1)
Active(owner=A) ──(owner A 续租)─────▶ Active(owner=A, generation+1)
Active(owner=A) ──(lease 超时)───────▶ Expired
Active(owner=A) ──(owner A 主动释放)─▶ Released / Missing
Active(owner=A) ──(owner B 抢占已过期 lease)──▶ Active(owner=B, generation+1)
```

不变量：同一 `workspace_id + controller_type` 同时最多一个有效 owner。

## Outbox 生命周期

```text
Pending ──(publisher claim)──────────────────────────▶ Publishing
Publishing ──(Redis XADD 成功并记录 stream_id)──────▶ Published
Publishing ──(Redis XADD 失败，可重试)───────────────▶ Pending
Publishing ──(重试耗尽)─────────────────────────────▶ Failed
Failed ──(人工或后台重试)────────────────────────────▶ Pending
```

规则：

- 聚合状态写入与 OutboxEvent 写入 MUST 在同一事务或同一原子写路径中完成。
- OutboxPublisher 可多实例运行，通过 MongoDB 原子 claim 防止重复发布。
- Redis XADD 成功但 MongoDB 标记 Published 失败时，publisher 可能重复发布；消费者 MUST 使用 idempotency_key 去重。

## Cooperative Cancel 协议

取消不依赖 RPC 或 Watch，使用 MongoDB 状态 + Redis control signal：

```text
1. 用户 REST POST .../cancel
2. API Server 写 cancel_requested_at，并写 OutboxEvent(CancelRequested)
3. OutboxPublisher 发布 Redis control signal / BoardEvent
4. Agent 在执行循环中检查：
   - Redis control stream
   - MongoDB WorkItem.cancel_requested_at / Project.cancel_requested_at
5. Agent 停止当前 SubAgent，清理 worktree/merge_lock
6. Agent 通过应用服务写 WorkItem/Project/Task Cancelled，并 XACK
7. 超过 cancel_timeout_sec 未确认时，Scheduler/reconciler 强制释放 lease 并标记 Cancelled 或 Pending 重试
```

竞态处理：ConfirmCancel 与 ForceCancel 都以 `cancel_requested_at` 非空、status 非终态、version/lease_owner 匹配作为前置条件。先到达者清零 `cancel_requested_at` 并写终态；后到达者幂等返回。

## BoardEvent / UI 更新状态

- Board snapshot 存于 MongoDB projection，通过 REST 查询。
- BoardEvent 经 Redis Stream 推送给 WebSocket gateway。
- 客户端重连携带 `last_stream_id`。
- Server 从 Redis BoardEvent stream 尝试补发；若 stream 已裁剪或检测到 gap，返回 `snapshot_required`，客户端重新 REST 拉取 snapshot。

BoardEvent 不参与领域状态机，NEVER 作为业务真相。
