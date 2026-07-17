# Task 端口与 Published Language

> 层级：02-modules / task（模块战术设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：#791（S2）/ #887 / #888 / #890 / [#972](https://github.com/rushsinging/aemeath/issues/972)

## 1. TaskAccess 与 TaskPersist

Task BC 从同一个 TaskStore backing instance 发布两个窄入站 OHS：Agent Runtime / TaskTool 只获得 `TaskAccess`，Context Management 的联合恢复协调器只获得 `TaskPersist`。两者 **NEVER** 合并成公开 super-trait；Runtime 因而无法在编译期调用高权限 restore commit。消费方也不接触 TaskStore、HashMap 或内部聚合方法。

### 1.1 端口定义

```rust
#[async_trait]
pub trait TaskAccess: Send + Sync {
    // ── CRUD ──
    async fn create_task(&self, spec: TaskCreateSpec) -> Result<TaskCommandResult<Task>, TaskCommandError>;
    async fn get(&self, id: &TaskId) -> Option<Task>;
    async fn transition(&self, id: &TaskId, to: TaskStatus) -> Result<TaskCommandResult<Task>, TaskCommandError>;
    async fn set_priority(&self, id: &TaskId, priority: TaskPriority) -> Result<TaskCommandResult<Task>, TaskCommandError>;
    async fn add_dependency(&self, id: &TaskId, blocked_by: &TaskId) -> Result<TaskCommandResult<Task>, TaskCommandError>;
    async fn remove_dependency(&self, id: &TaskId, blocked_by: &TaskId) -> Result<TaskCommandResult<Task>, TaskCommandError>;
    async fn add_tag(&self, id: &TaskId, tag: String) -> Result<TaskCommandResult<Task>, TaskCommandError>;
    async fn remove_tag(&self, id: &TaskId, tag: &str) -> Result<TaskCommandResult<Task>, TaskCommandError>;
    /// 在一次 state mutation 中移除该 Task 的全部依赖边，再标记 Deleted
    async fn delete(&self, id: &TaskId) -> Result<TaskCommandResult<Task>, TaskCommandError>;

    // ── 依赖图只读查询；写入命令内部仍 MUST 自行复核 ──
    async fn is_blocked(&self, id: &TaskId) -> bool;
    async fn would_create_cycle(&self, id: &TaskId, blocked_by_id: &TaskId) -> bool;

    // ── 批次 ──
    async fn create_batch(&self, spec: BatchCreateSpec) -> Result<TaskCommandResult<Batch>, TaskCommandError>;
    async fn pause_batch(&self, id: &BatchId) -> Result<TaskCommandResult<Batch>, TaskCommandError>;
    async fn resume_batch(&self, id: &BatchId) -> Result<TaskCommandResult<Batch>, TaskCommandError>;
    async fn archive_batch(&self, id: &BatchId) -> Result<TaskCommandResult<Batch>, TaskCommandError>;
    /// 每个 Runtime turn 结束时原子更新 Active Batch 的 last_active_turn / silence_turns。
    async fn record_batch_turn(&self, id: &BatchId, turn: u64, active: bool)
        -> Result<TaskCommandResult<Batch>, TaskCommandError>;

    // ── 查询 ──
    async fn list(&self) -> Vec<Task>;
    async fn list_batches(&self) -> Vec<Batch>;
    async fn lifecycle_snapshot(&self, stale_after_silence_turns: u64) -> TaskLifecycleSnapshot;
    async fn stats(&self) -> TaskStoreStats;
    async fn reminder_snapshot(&self) -> TaskReminderSnapshot;

}

pub struct TaskCreateSpec {
    subject: String,
    description: String,
    active_form: Option<String>,
    priority: TaskPriority,
}

pub struct BatchCreateSpec {
    summary: String,
}

impl TaskCreateSpec {
    pub fn try_new(
        subject: String,
        description: String,
        active_form: Option<String>,
        priority: TaskPriority,
    ) -> Result<Self, TaskCommandError>;
}

impl BatchCreateSpec {
    pub fn try_new(summary: String) -> Result<Self, TaskCommandError>;
}

pub trait TaskPersist: Send + Sync {
    fn collect_snapshot(&self) -> TaskSnapshot;
    /// 完整校验快照并构造不透明令牌；NEVER 修改 live state
    fn prepare_restore(
        &self,
        snapshot: &TaskSnapshot,
    ) -> Result<PreparedTaskRestore, TaskRestoreError>;
    /// 在 session-switch gate 内消费已验证令牌；MUST 无失败、无取消点
    fn commit_restore(&self, prepared: PreparedTaskRestore);
}

pub enum TaskCommandError {
    InvalidTaskSubject,
    InvalidBatchSummary,
    TaskIdExhausted,
    BatchIdExhausted,
    RevisionExhausted,
    IllegalTransition { from: TaskStatus, to: TaskStatus },
    DeletedOnlyViaDelete,
    IllegalBatchTransition { id: BatchId, from: BatchStatus, to: BatchStatus },
    TaskNotFound { id: TaskId },
    BatchNotFound { id: BatchId },
    NoActiveBatch,
    DependencyCycle { task_id: TaskId, blocked_by_id: TaskId },
    CrossBatchDependency { task_id: TaskId, blocked_by_id: TaskId },
    TaskBlocked { id: TaskId, blocked_by: Vec<TaskId> },
    ActiveBatchConflict { active: BatchId, requested: BatchId },
    BatchNotActive { id: BatchId, status: BatchStatus },
}

pub struct TaskCommandResult<T> {
    pub value: T,
    pub events: Vec<TaskEvent>, // 与本次成功 mutation 原子产生、顺序稳定
    revision: Option<TaskRevision>, // 私有；只经 revision() 读取
}

impl<T> TaskCommandResult<T> {
    pub fn revision(&self) -> Option<TaskRevision>;
}
```

`TaskRevision` 是 Task backing 的单调提交版本，空 store 从 `0` 开始。每次实际改变 `TaskStoreState` 的成功命令只递增一次，即使命令原子修改多个 Task、Batch 或依赖边；此时 `TaskCommandResult::revision()` 返回提交后的权威 `Some(TaskRevision)`。失败命令、只读查询和幂等 no-op 均不递增；聚合内尚未提交的本地结果与 store 幂等 no-op 的 `revision()` 均返回 `None`。`revision` 字段及其写入能力保持 Task-private，外部调用方只能通过 accessor 读取，不能伪造提交信息。真实提交的 revision 与命令的 `value` / `events` 在同一 state transaction 中产生，调用方 **NEVER** 通过命令后另查 revision 拼接提交结果。revision 溢出返回结构化 `RevisionExhausted`，state、events 与 revision 全部不变。`#887` 不引入 expected-revision/CAS 参数；若未来出现多 writer 乐观并发需求，必须另行设计冲突协议。

所有 mutation command 都返回 `TaskCommandResult<T>`；Runtime 必须先消费 `events` 并经 ACL 投影，再使用 `value` 更新本地 read model。失败没有事件。`add_dependency` 即使调用方先查询 `would_create_cycle`，也 **MUST** 在同一 store write mutation 内重新检查环；`transition(id, InProgress)` 同理必须锁内检查 blocked，查询方法只是 UI/规划提示，不能充当写入 precondition。

`TaskCreateSpec` / `BatchCreateSpec` 是 Task-owned typed command input，**NEVER** 与 Tool wire DTO 共用类型。其字段与直接构造器保持私有：`try_new` 校验非空 subject / summary；`create_task` / `create_batch` 再校验当前 state，最后才分配 ID 并在一次 state mutation 中插入。任何校验或 ID 溢出都返回结构化 `TaskCommandError` 且 state / counter 不变，**NEVER** 以 panic、空字符串修补或半创建表示失败。

`Task.batch` 是非可选引用，因此 `create_task` **MUST** 把新 Task 绑定到 `current_batch` 指向的唯一 `Active` Batch；合法空 snapshot 的 `current_batch=None` 时返回 `NoActiveBatch`，**NEVER** 隐式创建不可见 Batch、写入 `BatchId(0)` 或返回缺少 batch 的 Task。调用方必须先显式 `create_batch`。`create_batch` 要求当前没有 Active Batch，否则返回 `ActiveBatchConflict { active, requested }`；新建命令只接收并持久化 `summary`，**NEVER** 发布 Batch 不拥有的 `description` 参数。

Batch lifecycle 命令全部按 id 且 fallible：`pause_batch(Active)` 原子变为 Paused 并清空 current；`resume_batch(Paused)` 仅在无其他 Active 时原子设为 Active/current；`archive_batch(Active|Paused)` 原子变为 Archived 并按需清空 current，重复 archive 幂等返回 Archived 实体。不存在、非法迁移或另一 Active 存在均返回 typed error 且不改 state。公开 Target **NEVER** 保留依赖隐式 current 的 `complete_batch()` shortcut。

`TaskReminderSnapshot` 是 Task-owned 只读 PL，只包含 `current_batch` 与按稳定顺序排列的 `TaskReminderItem { id, subject, status, blocked }`；不含 store handle、依赖图内部缓存或渲染文本。Runtime `context_coordination` 可读取该纯值后传入 `ContextRequest`，Context Management 独占 reminder 的格式、位置与 token budget。

### 1.2 恢复协议类型（#888 / #890，非 #887 范围）

`#887` 只建立 `TaskAccess`、revision 与单一事务 backing，不实现 snapshot codec、restore 校验、`TaskPersist` 或 Context/Composition 接线。下列恢复协议由 `#888` 定义版本化快照与安全校验，由 `#890` 发布 collect / prepare / commit 能力并从同一 backing 分发高权限 view。

```rust
#[must_use]
pub struct PreparedTaskRestore { /* private: 完整 TaskStoreState */ }

pub enum TaskRestoreError {
    DuplicateTaskId { id: TaskId },
    DuplicateBatchId { id: BatchId },
    DanglingDependency { task_id: TaskId, dependency_id: TaskId },
    DependencyCycle,
    PersistedDeletedTask { id: TaskId },
    InvalidBatchReference { task_id: TaskId, batch_id: BatchId },
    InvalidCurrentBatch { batch_id: BatchId },
    InvalidNextTaskId,
    InvalidNextBatchId,
}

pub enum TaskSnapshotDecodeError {
    InvalidTaskIdFormat,
    InvalidBatchIdFormat,
    MixedIdFormat,
    InvalidLegacyCounter,
    MalformedSnapshot,
}

impl TaskSnapshot {
    /// Task-owned wire ACL：raw JSON → typed snapshot；Context 不解析内部 ID。
    pub fn decode_wire(raw: &serde_json::value::RawValue)
        -> Result<Self, TaskSnapshotDecodeError>;
    pub fn empty() -> Self;
}
```

`PreparedTaskRestore` 的类型名因 `TaskPersist` 跨 crate 消费而公开，但字段与构造器 **MUST** 保持 Task-private；它 **NEVER** 实现 `Clone`、`Serialize` 或 `Deserialize`，只允许 `commit_restore` 按值消费一次，因此不是持久化 Published Language。`TaskRestoreError` 是 prepare 阶段的结构化协议错误，**NEVER** 以字符串替代错误类别。

### 1.3 端口设计原则

| 原则 | 说明 |
|---|---|
| **返回值用 PL 类型** | 所有返回值是 `Task`、`TaskSnapshot` 等 Published Language 类型，不泄漏内部结构 |
| **不暴露 store** | 调用方拿不到 `&TaskStore` 或 `&HashMap` |
| **只发布意图命令** | `TaskAccess` 只接受 transition / priority / dependency / tag / delete 等目的性命令，**NEVER** 向调用方交出 `&mut Task` 或公开字段写权限 |
| **能力分离** | `TaskAccess` 只含日常命令 / 查询；`TaskPersist` 独占 snapshot 与 restore，二者来自同一 backing instance |
| **快照经窄端口** | `collect_snapshot` / `prepare_restore` / `commit_restore` 是跨 BC 快照组装与原子恢复的唯一入口 |
| **准备不变更 live state** | `PreparedTaskRestore` 是 Task-owned、不可伪造、一次性消费的 opaque token；prepare 失败时旧 Task 集合保持原样 |
| **提交无失败** | Context Management 持有排他 session-switch gate 后调用同步 `commit_restore`；token 已验证，commit 无 I/O、无 await、无错误返回 |

### 1.4 消费方

| 消费方 | 使用方式 |
|---|---|
| **Agent Runtime / TaskTool** | 只获得 `TaskAccess`：创建 / 推进 / 删除 Task；`is_blocked` 仅用于展示/规划提示，真正执行 admission 必须直接调用原子 `transition(id, InProgress)` 并处理 `TaskBlocked` |
| **Context Management** | 只获得 `TaskPersist`：Session 落盘时 collect；恢复时先取得 exclusive session-switch lease，再在同一 lease 内与 Project 一起 prepare / commit |
| **TUI** | 经 SDK 事件投影 Task 状态，**NEVER** 直接调用 Task OHS |

## 2. Task Published Language

### 2.1 PL 类型清单

Context Map §7 和 §10 决策：Task 类型是 Task BC 的 Published Language（非 Shared Kernel），由 Task BC 独占不变量，其他 BC 引用其发布类型。

| PL 类型 | 说明 | 消费方 |
|---|---|---|
| `Task` | 运行期命令 / 查询 read model；可用于 SDK 事件，但含派生 `blocks`，**NEVER** 用作 Session 持久化 DTO | Runtime、TUI（经事件） |
| `TaskId` / `BatchId` | Task-owned 强类型标识；wire 为单 Session 十进制数字字符串 | Runtime、Context Management、TUI（经 SDK DTO） |
| `TaskStatus` | 状态枚举（Pending / InProgress / Completed / Deleted） | Runtime、TUI |
| `TaskPriority` | 优先级枚举（Low / Normal / High / Urgent） | Runtime、TUI |
| `TaskCreateSpec` / `BatchCreateSpec` | Task-owned typed/fallible 新建命令；与 Tool wire DTO 分离 | Runtime、TaskTool ACL |
| `TaskCommandResult<T>` / `TaskEvent` | mutation 的原子结果与封闭领域事件；Runtime ACL 投影到 SDK | Runtime、TaskTool ACL |
| `TaskCommandError` | 日常命令的封闭结构化失败类型 | Runtime、TaskTool ACL |
| `Batch` | `TaskStoreState` 聚合内的批次实体 | Runtime、TUI |
| `BatchStatus` | 批次状态枚举（Active / Paused / Archived） | Runtime、TUI |
| `TaskReminderSnapshot` / `TaskReminderItem` | 稳定排序、无渲染文本的 Context reminder 输入 | Runtime → Context Management |
| `TaskLifecycleSnapshot` | 带显式 stale threshold 的 Batch lifecycle 纯值输入 | Runtime |
| `TaskSnapshot` | 可持久化快照 | Context Management |
| `PersistedTask` | `TaskSnapshot` 内部持久化 DTO；不含派生 `blocks` | Context Management（只随 snapshot） |
| `TaskStoreStats` | 统计信息；只能经 Runtime→SDK DTO 投影给 TUI | Runtime |

### 2.2 PL 所有权与发布边界

| 边界 | 角色 | 说明 |
|---|---|---|
| **Task capability root 的受控 façade** | 所有权属 | 类型定义的权威来源；不规定独立物理目录 |
| **`packages/sdk`** | 契约 crate | SDK 中引用 Task PL 类型，供 TUI / Server 消费 |

`build.rs` 与 SDK **MUST** 从 Task BC 的同一类型定义生成或引用 JSON Schema；**NEVER** 在 `shared`、SDK 与 Task 三处复制 schema 或领域类型。若编译期工具需要独立依赖边界，Task 必须发布受控 re-export 或生成产物，而不是转移所有权。具体是与 `task.rs` 共置还是按证据展开 `model.rs`，只由 [代码组织规范](../../01-system/06-code-organization.md) 的判据决定。

### 2.3 序列化约定

- 需要进入 SDK event 的 read model 可实现 serde；Session 持久化只编码 `TaskSnapshot` / `PersistedTask`，**NEVER** 直接编码运行时 `Task`。运行时 opaque token 不实现 serde。
- 枚举使用 `#[serde(rename_all = "snake_case")]`。
- 向前兼容：新增字段使用 `#[serde(default)]`。
- v0.1.0 ID 使用十进制数字字符串。Task-owned `TaskSnapshot::decode_wire` 可接受“全部新字符串”或“全部 legacy 数字”并转换为 typed snapshot，但 **MUST** 拒绝 invalid / mixed format；Context 的 Session reader只委托 codec，不解析 Task / Batch ID。

## 3. TaskStore 与 Session 持久化边界

### 3.1 分层关系

```
TaskAccess + TaskPersist            ← 同一 Task-owned backing 的两个窄 OHS
  └── TaskStore (in-memory backing)  ← 同时实现两者，只持有单一 TaskStoreState

Context Session Repository ──消费 TaskSnapshot──▶ Storage 原子写机制
```

### 3.2 TaskStore 定位

TaskStore 是 Task BC 的内存 backing：

- **持有单一 state slot**：全部可变字段 **MUST** 收进一个 `TaskStoreState { tasks, next_task_id, next_batch_id, current_batch, batches }`，由一把同步 lock / 一次替换守护；**NEVER** 为这些字段分别建锁。
- **实现两个窄 OHS**：`TaskStore` 同时实现 `TaskAccess` 与 `TaskPersist`，但 production wiring **MUST** 分别分发 view，**NEVER** 向 Runtime / Tool 暴露 Persist。
- **不执行文件 I/O**：TaskStore 只发布 / 恢复 snapshot；Context Management 的 Session repository 才把 snapshot 内嵌 Session 并调用 Storage 原子写机制。
- **数据所有权不变**：Task 数据本体与 schema 归 Task BC，Storage 只保存 Session repository 提交的 bytes，**NEVER** 形成第二条 Task 文件路径。

所有 async CRUD **MUST** 在持锁前完成 await / I/O，且 **NEVER** 跨 await 持有 state guard。`TaskPersist` 的 collect / prepare / commit 均同步且无 I/O：prepare 在独立值上构造完整 `TaskStoreState`，commit 只获取一次同步写锁并执行一次 state swap。Main Run admission、Task 查询/Tool 与 resume commit **MUST** 受同一个 session-switch gate 协调；Sub 使用自己的 isolated state slot，不参与 Main restore。

### 3.3 Production wiring

crate-root 窄 façade `task::wire_task() -> TaskWiring` **MUST** 返回字段私有、仅 Composition 可消费的 opaque handle；`access()` 与 `persist()` 分别返回同一 TaskStore backing 的 `Arc<dyn TaskAccess>` / `Arc<dyn TaskPersist>`。这不是要求新增通用 `api/` 层。`TaskWiring` **NEVER** 进入 Runtime、Tool、Context 的业务类型，且架构守卫 **MUST** 限制 `persist()` 只接线到 Context Management 的 Main session coordinator。Sub 可新建独立 wiring，但只把 Access 注入 Sub Runtime / Tool；Sub 不参与 Main Session restore。

### 3.4 持久化模型

Task 的持久化不独立落盘，而是通过 Session 快照内嵌：

1. Context Management 触发 Session 落盘。
2. 经 `TaskPersist::collect_snapshot()` 获取 `TaskSnapshot`。
3. `TaskSnapshot` 嵌入 Session 持久化 DTO。
4. Storage 将 Session DTO 原子写入 `~/.agents/sessions/`。
5. 恢复时反向：读取 Session DTO → 兼容 ACL 将 absent legacy 升级为规范空快照、保留 captured empty 原值 → 在同一 exclusive lease 内依序 prepare Project → Config → Memory → Task；四者全部成功后才进入无失败 commit。

> **Decision**：Task **NEVER** 独立驱动持久化。所有落盘经 Session 快照路径，确保 Task 与 Session 状态一致。

## 4. 与 Context Management 的集成

### 4.1 快照组装

Session 落盘时的快照组装是 Context Map §8 定义的关键 ACL 位置之一：

```
Context Management                Task BC
     │                               │
     │  collect_snapshot()           │
     │ ────────────────────────────▶ │
     │                               │ 返回 TaskSnapshot
     │ ◀──────────────────────────── │
     │                               │
     │  嵌入 Session DTO             │
     │  写入磁盘                     │
```

### 4.2 快照恢复

```
Context Management                Task BC
     │                               │
     │  读取 Session DTO              │
     │  提取 TaskSnapshot             │
     │                               │
     │  prepare_restore(snapshot)    │
     │ ────────────────────────────▶ │
     │                               │ 完整校验；live state 不变
     │ ◀──────────────────────────── │
     │  Prepared token / Err         │
     │                               │
     │  commit_restore(token)        │
     │ ────────────────────────────▶ │
     │                               │ gate 内无失败全量替换
```

### 4.3 边界约束

- Context Management **NEVER** 直接操作 Task 内部状态。
- 快照组装经 `TaskPersist`，不共享内部结构；Runtime / Tool 只持 `TaskAccess`，编译期不可调用 commit。
- raw wire **MUST** 先经 Task-owned codec 转成 typed `TaskSnapshot`；格式、legacy number → newtype 与 mixed-format 错误只在该阶段产生。`prepare_restore` **MUST** 只校验 typed Task / Batch ID 唯一性、拒绝 persisted `Deleted` tombstone、依赖引用、依赖环、每个 Task 的 batch 引用、`current_batch`、`next_task_id` 与 `next_batch_id` 一致性，且 **NEVER** 修改 live state。
- `blocked_by` 是持久化依赖事实，`blocks` 只是反向投影。prepare **MUST** 忽略 wire 中可能存在的旧 `blocks` 值，并从已验证的 `blocked_by` 在 candidate state 内完整重建，**NEVER** 把损坏的反向投影提交到 live state；新 snapshot writer **MUST** 通过 `PersistedTask` 省略该字段，**NEVER** 直接 serde 运行时 `Task`。
- `commit_restore` 是全量替换，不做增量合并；captured empty **MUST** 清空旧任务。兼容 reader 遇到 absent legacy **MUST** 调用 Task-owned `TaskSnapshot::empty()` 构造 `tasks=[] / next_task_id=TaskId(1) / next_batch_id=BatchId(1) / current_batch=None / batches=[]`，记录 `LegacyTaskSnapshotAbsent` 诊断，再走同一 prepare / commit；它 **NEVER** 保留当前 Session 的旧 Task，也 **NEVER** 把兼容来源写入 live Task state。legacy snapshot 缺 `next_batch_id` 时，codec 以最大 BatchId + 1 派生并检查溢出。
- 新 writer **MUST** 始终写出 `Some(TaskSnapshot)`，即使没有任务也写 captured empty；`None` 只允许出现在 legacy wire DTO reader。
- `collect_snapshot` **MUST** 满足 round-trip：任一合法 live Task state 产生的快照可被同版本 `prepare_restore` 接受。`delete` 在过滤 tombstone 前必须从所有活 Task 移除双向依赖边，**NEVER** 生成 dangling reference。
- Project → Config → Memory → Task 四个 participant 的 prepare / commit **MUST** 只由 Context Management 的联合恢复协调器在同一个已持有的 exclusive session-switch lease 内调用；prepare token 生成与 commit 之间 **NEVER** 释放 lease。

## 5. 与 Agent Runtime 的集成

### 5.1 Runtime 消费模式

Agent Runtime 通过 `TaskAccess` 管理 Task 作为自身执行规划的投影：

| Runtime 动作 | TaskAccess 调用 |
|---|---|
| 开始多步工作 | `BatchCreateSpec::try_new` → `create_batch`；`TaskCreateSpec::try_new` → `create_task` × N，逐次处理 typed error |
| 开始执行某任务 | `transition(id, InProgress)` |
| 任务完成 | `transition(id, Completed)` |
| 添加依赖 | `add_dependency(id, blocked_by)` |
| 检查 blocked 展示/规划提示 | `is_blocked(id)`；**NEVER** 作为随后 transition 的 precondition |
| 用户中断 / 新话题 | `pause_batch(batch_id)`，随后可创建新 Batch |
| 恢复旧批次 | 在无其他 Active 时 `resume_batch(batch_id)` |
| 批次完成 / 废弃 | `archive_batch(batch_id)` |

### 5.2 边界约束

- Runtime **NEVER** 守护 Task 状态机不变量——不变量由 Task BC 独占。
- Runtime **NEVER** 直接读写 Task 字段，也拿不到 `&mut Task`；只调用 `TaskAccess` 的目的性命令。
- Runtime 通过领域事件投影 Task 状态到 TUI，TUI 不自行推导。

## 6. 相关文档

- Task 领域模型：[01-domain-model.md](01-domain-model.md)
- 模块入口：[README.md](README.md)
- Context Map：[../../01-system/03-context-map.md](../../01-system/03-context-map.md) §4 / §6 / §7 / §8 / §10
- Runtime 端口与装配：[../runtime/06-ports-and-adapters.md](../runtime/06-ports-and-adapters.md)
- Context Management Session：[../context-management/01-session.md](../context-management/01-session.md)

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-12 | 初稿：Task 访问契约、PL 清单与发布位置、Storage 集成、跨 BC 快照组装 | #791 |
| 2026-07-14 | 拆分 TaskAccess / TaskPersist；快照恢复改为无副作用 prepare + gate 内无失败 commit，并明确 absent legacy / captured empty | [#972](https://github.com/rushsinging/aemeath/issues/972) |
| 2026-07-14 | 新建命令改为 typed/fallible spec，补齐 Batch pause/resume/archive-by-id 并移除 current-only complete shortcut | [#972](https://github.com/rushsinging/aemeath/issues/972) |
