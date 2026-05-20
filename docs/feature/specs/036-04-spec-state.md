# #36 多 Agent 框架 — Spec / 状态机设计

状态枚举 Rust 侧使用 PascalCase，序列化到 MongoDB 使用 snake_case（通过 `#[serde(rename_all = "snake_case")]`）。

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
    HeartbeatLost,  // 心跳丢失（恢复后 → Idle）
    Error,          // 异常状态（暂时性 → 冷却恢复 Idle；持久性 → Scheduler 回收文档）
}
```

说明：销毁（删除 AgentInstance 文档）即 Agent 销毁流程，无需单独的 `Destroyed` 终态。

## 状态流转

### Project 状态流转
```
Scheduler Watch 到 Pending Project
     │
     ▼
  Pending ──(分配 Executor)──▶ Assigned
     │                         │
     │                         ├── 用户取消（通知 Executor）──▶ Cancelled
     │                         │
     │                         ├── Scheduler 对账：status=assigned && assigned_at < now - assign_timeout_sec
     │                         │   → Pending（清理分配信息并回退）
     │                         │   （若 Executor 崩溃且超时，由 Scheduler 对账检测；不需要独立的 Assigned→Failed 路径）
     │                         │
     │                         └── Executor 开始执行
     │                             ▼
     │                          InProgress ──▶ Pending（崩溃恢复；下属非终态 ProjectTask 一并回退到 Pending）
     │                             │
     │                             ├── Executor 调用 ProjectService.Complete RPC；API Server 校验所有 Task 终态后原子写入 ──▶ Completed
     │                             │
     │                             ├── Executor 调用 ProjectService.ReportBlocked(code_blocked RPC，带 block_reason) ──▶ Blocked ──(反馈写入 ChatMessage 并 Resume：POST .../projects/:id/resume)──▶ InProgress
     │                             │                     │
     │                             │                     ├── 用户取消 ──▶ Cancelled（释放 merge_lock）
     │                             │                     │
     │                             │                     └── 超时（blocked_timeout_sec，默认 3600s，由 Scheduler 对账循环每 reconcile_interval_sec 检测；配置见 036-02 scheduler.*）──▶ Failed
     │                             │                                         （释放 merge_lock；Project 在 Blocked 期间持续持有 merge_lock，
     │                             │                                          超时后强制释放。级联行为：该 Project 下所有非终态且非 Pending 的 Task
     │                             │                                          （InProgress / InReview / Retrying）→ Failed；**Pending Task 保持 Pending**
     │                             │                                          ——因尚未执行，不应标记 Failed，允许分配给新 Executor 继续。
     │                             │                                          ⚠ Project 已是 Failed 终态，下属 Pending Task 无法被 Scheduler 调度（不再满足"Project 处于可调度状态"的前置条件），成为孤儿数据。当 Project 通过人工重开（Failed→Pending）恢复后，Pending Task 可重新参与调度。若 Project 永不重开，孤儿 Task 由后台 GC 定期清理或标记。）
     │                             │
     │                             ├── 用户取消（cooperative cancel，释放 worktree/merge_lock）──▶ Cancelled
     │                             │
     │                             └── 执行失败 ──▶ Failed（需人工干预重开）
     │                                 │（释放 merge_lock；级联：同级联 Project 下所有非终态且非 Pending 的 Task → Failed；Pending Task 保持 Pending）
     │                                 │
     │                                 └── 人工重开 ──▶ Pending
     │
     └── 用户取消 / 级联取消 ─────────────────────────────▶ Cancelled

    - Pending → Cancelled：用户取消或 Requirement 级联取消
    - Assigned → Cancelled：用户取消（Assigned 阶段无需 Cooperative Cancel；API Server 直接设 status=Cancelled，Executor Watch 到 assigned_project_id 已被取消则忽略 Assign）
    - InProgress → Cancelled：用户取消；Executor 采用 cooperative cancel，停止当前执行并释放 worktree / merge_lock
    - Blocked → Cancelled：用户取消，适用于长时间无法解决的阻塞
    - **级联规则**：Project 进入 Cancelled 终态时，该 Project 下所有非终态的 Task（Pending / InProgress / InReview / Retrying）一律级联为 Cancelled。**执行者**：由 ConfirmCancel/ForceCancel RPC handler 在设 Project status=cancelled 的同一事务中，用 `update_many({ project_id, status: { $nin: [Completed, Failed, Cancelled] } }, { $set: { status: "cancelled" } })` 原子批量写入（使用 MongoDB 多文档事务保证原子性）。已终态 Task 不受影响。Pending Task 需要被级联取消，因为 Project 不再可调度。
    - **Cancelled → Pending 重开**：用户可人工重开已取消的 Project（仅允许 Cancelled → Pending），下属已 Cancelled Task 由用户决定是否一并恢复。注意区别于 `Failed → Pending`（Retry）——取消是外部意图（不一定是执行失败），重开后的处理策略相同。
```

### Requirement 状态流转

```
Pending ──▶ Analyzing（Assistant 原子抢占）
Pending ──▶ Cancelled（用户取消）
Analyzing ──▶ Draft（草案产出，可被确认）
Analyzing ──▶ Pending（超时回退 / 冲突放弃）
Analyzing ──▶ Cancelled（分析中取消）

Draft ──▶ Draft（用户驳回后选择重新生成：POST .../reject { regenerate: true } → Requirement 保持 Draft，Scheduler 重新调度 Assistant 产出新草案；允许多轮 Draft）
Draft ──▶ InProgress（用户确认；前提：Confirm RPC 事务性写入所有 draft 中的 Project + ProjectTask 文档，全部成功后 Requirement 入 InProgress；任一写入失败则 Requirement 保持 Draft，已写入文档由调用方补偿回滚）
Draft ──▶ Rejected（用户驳回并选择放弃，不触发重新生成）
Draft ──▶ Cancelled（用户取消）
Rejected ──▶ Analyzing（用户重新提交）

InProgress ──▶ Completed（由 API Server 在 Project→Completed 时级联检查：该 Requirement 下所有 Project 均为终态 → 自动设 Requirement.status=Completed）
InProgress ──▶ Cancelled（用户取消）
  
> **Requirement InProgress → Cancelled 级联规则**：
> - 关联的非终态 Project 一律级联为 Cancelled（其中 InProgress / Blocked 的 Project 走 Cooperative Cancel 协议，参见 § Cooperative Cancel）
> - 已处于终态（Completed / Failed / Cancelled）的 Project 不受影响
```

### ProjectTask 状态流转

```
pending ──▶ in_progress（Executor 分配给 Sub-Agent 并开始执行）
in_progress ──▶ in_review
in_progress ──▶ completed（仅无 Review 需求的 Task：executor_type=sequential 且所有 Sub-Agent 通过）——见下方说明
in_progress ──▶ failed（执行失败）
in_progress ──▶ retrying（Executor 正常自动重试）──▶ pending（短暂冷却后重新分配）──▶ in_progress
in_review ──▶ completed / failed（产出 / 阻断）
in_review ──▶ in_progress（返工）
in_review ──▶ retrying（review 阶段判需重试，与 in_progress→retrying 共享 retry_count 上限）──▶ pending（冷却后重新分配）──▶ in_progress
retrying ──▶ failed（重试耗尽：max_task_retries（默认 3，定义见 Data spec ProjectTask.max_task_retries）次后仍失败或不可重试失败）
failed ──▶ pending（人工重开：POST .../tasks/:id/retry；v0.1 若无此 API 则需创建新 Project/Task）
pending / in_progress / in_review / retrying ──▶ cancelled（用户取消；排除 Completed/Cancelled/Failed 终态）
cancelled ──▶ pending（人工重开：仅在所属 Project 非 Cancelled 终态时允许；若 Project 也被取消，需先重开 Project）

// 崩溃恢复路径（Scheduler 心跳超时检测 → 级联回退，不在正常流转图中）:
in_progress ──▶ pending（Executor 崩溃恢复，清空 assigned_executor_id）
in_review   ──▶ pending（Executor 崩溃恢复，清空 assigned_executor_id）
retrying    ──▶ pending（Executor 崩溃恢复，保留 retry_count + last_error）
```

### Cooperative Cancel 协议

Project `InProgress → Cancelled` / ProjectTask `* → cancelled` 触发流程：

> 说明：Executor 是 gRPC client（调用 API Server），非 server。取消信号通过 Watch / Change Stream 的 pull 模型传递，而非 Server→Executor 的 push 模型。
> Cooperative Cancel 两阶段模型：阶段 1 — REST 端仅设置 `cancel_requested_at`（非 `status`），不立即改变状态；阶段 2 — Executor Watch 感知后主动停止并回调 API Server 确认，届时才设置 `status = cancelled` 并清零 `cancel_requested_at`。

1. 用户通过 REST `POST .../cancel` 发起取消 → **API Server 的 ProjectService / ProjectTaskService 写入 `cancel_requested_at` 时间戳**（⚠ 此时不改变 `status`，仍为 InProgress / Blocked）。该字段由 API Server 的 REST handler 直接写入，不依赖 Scheduler 同步参与。
2. Executor 的 `Watch` stream 推送文档变更（`cancel_requested_at` 变为非空 → Executor 感知取消请求）
3. Executor 在每步 Sub-Agent 调用间检查 `cancel_requested_at`。检测到取消后：停止当前 Sub-Agent → 释放 merge_lock → 清理 worktree → 通过 `ProjectService.ConfirmCancel`（或 `ProjectTaskService.ConfirmCancel`）RPC 回调 API Server，由 API Server 原子地将 `status` 设为 `cancelled` 并将 `cancel_requested_at` 清零
  > 单文档 findAndModify 天然原子，无需 MongoDB 多文档事务。
4. 强制超时：从 `cancel_requested_at` 字段时间戳起算，经过 `cancel_timeout_sec`（默认 60s，配置见 036-02 `scheduler.*` 段）后若 Executor 未回调确认，由 Scheduler 调用 `ProjectService.ForceCancel`（或 `ProjectTaskService.ForceCancel`）RPC，由 API Server 原子执行：设 `status=cancelled`、清零 `cancel_requested_at`、释放 merge_lock、级联取消所有非终态 Task（参见上文级联规则）
  
> **竞态处理**：ConfirmCancel 和 ForceCancel 均以 `cancel_requested_at` 非空作为执行前置条件（findAndModify 原子检查）。先到达者将 `cancel_requested_at` 清零 + 设 `status=cancelled`；后到达者因 `cancel_requested_at` 已为空，操作幂等返回 `ok=false`。确保不会重复设 status。

### AgentInstance 生命周期
```
Scheduler 创建 Agent
     │
     ▼
Initializing ──(初始化成功)────────────────▶ Idle
Initializing ──(初始化失败 / 超时 agent_init_timeout_sec，默认 30s，配置见 036-02 scheduler.*）──▶ Error
Idle         ──(领取任务)─────────────────▶ Busy
Idle         ──(心跳超时)─────────────────▶ HeartbeatLost
Busy         ──(任务完成)─────────────────▶ Idle
Busy         ──(任务异常终止/panic)────────▶ Error
Busy         ──(心跳超时)─────────────────▶ HeartbeatLost
HeartbeatLost──(心跳恢复)─────────────────▶ Idle
HeartbeatLost──(恢复失败/异常升级)─────────▶ Error
Error        ──(冷却恢复/自动重启)─────────▶ Idle
Error        ──(持久异常：Scheduler 销毁旧 AgentInstance → 重新创建)──▶ [销毁] → Initializing → Idle
Error        ──(持久异常，非恢复类)─────────▶ [销毁]
  
> **Error 恢复策略说明**：暂时性错误（模型超时/熔断）走 `Error→Idle` 原地恢复；持久性错误（配置无法解析、资源耗尽）由 Scheduler 销毁旧 AgentInstance 文档后重新创建，走完整的 `Initializing→Idle` 路径。

> **⚠ HeartbeatLost 恢复竞态处理**：Agent 从 HeartbeatLost 恢复心跳时（HeartbeatLost→Idle），Scheduler 可能已在当前对账周期开始崩溃恢复（释放 Project、回退 Task）。恢复后的 Agent 必须在转为 Idle 后校验：
> 1. 自身 `current_project_id` 是否已被清空（若已被 Scheduler 释放则 Agent 主动清理本地 worktree/merge_lock）
> 2. 对比 `agent_heartbeats` 表中 `heartbeat_at` 与自身发起的最后心跳时间，判断断连窗口长度
> 3. 如果 Scheduler 已替其回退 Task（Task.assigned_executor_id 变为 null），Agent **不得**继续执行
> 4. 时序说明：`heartbeat_timeout_sec=30s` 后 Scheduler 标记 HeartbeatLost，Scheduler 在下一个对账周期（由 `reconcile_interval_sec=5s` 驱动，即至多延迟 5s 后执行）开始崩溃恢复。Agent 在两次心跳之间（T0+30s 到 ≤ T0+35s）恢复心跳成功即可避免 Task 被回退。
```

Initializing 退出条件：Agent 注册成功 → Idle；超时 30s（如连接 DB 失败或依赖初始化未完成）→ Error。

**Chat Agent 特殊说明**：Chat 创建 AgentInstance 文档（由连接层注册，非 Scheduler 分配），遵循 AgentStatus 状态机。Idle = 等待用户消息，Busy = 处理消息并写入白板。

Chat Agent 使用**双通道健康模型**：
- **WS keepalive（连接通道）**：由 WS 连接层处理。WS 断开后经短暂容忍窗口（WS graceful close，约 5s）触发 AgentInstance doc 直接删除（跳过 HeartbeatLost→Error 路径）。适用于 Chat Agent 崩溃/网络断开场景。
- **gRPC 逻辑心跳（Scheduler 通道）**：Chat Agent 通过 `AgentRegistryService.Heartbeat` RPC 定期向 `agent_heartbeats` 表写入心跳。Scheduler 仅在 Chat Agent 为 **Busy** 状态时监控此心跳，用于检测"WS 活跃但内部卡死"场景（如 LLM 调用无限等待）。若 Busy 持续超过 `busy_timeout_sec`（默认 600s），Scheduler 标记 AgentStatus→Error。
- WS 断开触发 Agent 删除，但 WS keepalive 时间 ≥ gRPC heartbeat 时间——两个通道互不冲突，组合覆盖全部故障场景。

HeartbeatLost→恢复失败→Error 路径仅适用于 Executor/Assistant 等 Scheduler 管理池内的 Agent（单通道 gRPC 心跳模型）。Chat Agent **不使用** HeartbeatLost 中间状态。
