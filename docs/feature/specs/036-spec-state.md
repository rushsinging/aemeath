# #36 多 Agent 框架 — Spec / 状态机

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

说明：销毁（删除 AgentInstance 文档）即 Agent 销毁流程，无需单独的 `Destroyed` 终态。

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
     │                  InProgress ──▶ Pending（崩溃恢复）
     │                     │
     │                     ├── 所有 ProjectTask 为 Completed 或 Cancelled ──▶ Completed
     │                     │
     │                     ├── 等待用户反馈 ──▶ Blocked ──(反馈写入 ChatMessage 并 Resume)──▶ InProgress
     │                     │                     │
     │                     │                     └── 用户取消（长时间无法解决）──▶ Cancelled
     │                     │
     │                     ├── 用户取消（cooperative cancel，释放 worktree/merge_lock）──▶ Cancelled
     │                     │
     │                     └── 执行失败 ──▶ Failed（终态，需人工干预重开）

    - Pending → Cancelled：用户取消或 Requirement 级联取消
    - Assigned → Cancelled：用户取消，需通知 Executor
    - InProgress → Cancelled：用户取消；Executor 采用 cooperative cancel，停止当前执行并释放 worktree / merge_lock
    - Blocked → Cancelled：用户取消，适用于长时间无法解决的阻塞
```

### Requirement 状态流转

```
pending ──▶ analyzing（Assistant 原子抢占）
analyzing ──▶ draft（草案产出，可被确认）
analyzing ──▶ pending（超时回退 / 冲突放弃）

draft ──▶ InProgress（用户确认并创建关联 Project）

InProgress ──▶ completed（所有 ProjectTask 为 Completed/Cancelled）
```

### ProjectTask 状态流转

```
pending ──▶ in_progress（Executor 分配给 Sub-Agent 并开始执行）
in_progress ──▶ in_review
in_progress ──▶ failed（执行失败）
in_review ──▶ completed / failed（产出 / 阻断）
in_review ──▶ in_progress（返工）
* ──▶ retrying（自动重试）──▶ pending
* ──▶ cancelled（用户取消）
```

### AgentInstance 生命周期
```
Scheduler 创建 Agent
     │
     ▼
Initializing ──(初始化成功)──────────────▶ Idle
Initializing ──(初始化失败)──────────────▶ Error
Idle         ──(领取任务)────────────────▶ Busy
Idle         ──(心跳超时)────────────────▶ Error
Busy         ──(任务完成)────────────────▶ Idle
Busy         ──(心跳超时)────────────────▶ Error
Error        ──(冷却恢复/自动重启)────────▶ Idle
Error        ──(Scheduler 回收)──────────▶ 销毁
```

Initializing 退出条件：Agent 注册成功 → Idle；超时 30s（如连接 DB 失败或依赖初始化未完成）→ Error。
