# Task 领域模型

> 层级：02-modules / task（模块战术设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：#791（S2）/ #890 / [#972](https://github.com/rushsinging/aemeath/issues/972)

## 1. TaskStoreState 聚合与 Task 实体

`TaskStoreState` 是 Task Management BC 的聚合根与单一一致性边界，封装全部 Task、Batch、双向依赖边、current_batch 与 ID 计数器。`Task` 是其中一个可追踪工作单元实体，拥有自身字段与局部迁移方法，但跨 Task / Batch 不变量只能由 `TaskStoreState` 的 store-backed command 在一次 mutation 内守护。

### 1.1 字段

| 字段 | 类型 | 说明 |
|---|---|---|
| `id` | `TaskId` | Task-owned newtype；wire 为十进制数字字符串，完整约束见 §6 |
| `subject` | `String` | 任务标题（简短一行） |
| `description` | `String` | 任务描述（多行） |
| `status` | `TaskStatus` | 状态机当前状态 |
| `active_form` | `Option<String>` | 进行时描述（spinner 显示用） |
| `blocked_by` | `Vec<TaskId>` | 依赖的 Task ID 列表 |
| `blocks` | `Vec<TaskId>` | 被此 Task 阻塞的 Task ID 列表（仅 live 反向投影） |
| `priority` | `TaskPriority` | 优先级（Low / Normal / High / Urgent） |
| `created_at` | `u64` | 创建时间戳（ms since epoch） |
| `updated_at` | `u64` | 最后更新时间戳 |
| `started_at` | `Option<u64>` | 首次开始执行时间；首次进入 InProgress 时设置，Pending 直接完成时与 completed_at 同时设置 |
| `completed_at` | `Option<u64>` | 完成时间；进入 Completed 时设置 |
| `session_id` | `Option<String>` | 所属 Session |
| `tags` | `Vec<String>` | 分类标签 |
| `batch` | `BatchId` | 所属 Batch ID |

> **边界决策（#885）**：Task 不拥有 `owner` / assignee 字段。任务分配和当前执行者属于 Agent Runtime 的执行绑定语义，应由 Runtime-owned `TaskAssignment { task_id, agent_id }`（或等价模型）关联 Task Published Language 与 Runtime `AgentId`。Task BC **NEVER** 引用 Agent 身份，也不把该绑定写入 Task Snapshot。现有 Tool/Storage 的 `owner: String` 仅为 legacy DTO 字段，由 #889 停止消费、#891 最终删除。

### 1.2 不变量

| # | 不变量 | 守护方式 |
|---|---|---|
| INV-1 | `id` 创建后不可变 | 聚合方法不接受 id 修改参数 |
| INV-2 | `status` 只能沿合法迁移路径变化 | 状态迁移经聚合方法 `transition_to()` 守护 |
| INV-3 | `blocked_by` 不成环 | 添加依赖前 DFS 环检测 |
| INV-4 | `blocks` = 全局 `blocked_by` 的反向投影 | 由聚合在 add / remove dependency 时同步维护，禁止独立写入 |
| INV-5 | `Deleted` 状态的 Task 不参与依赖图且不被任何活 Task 引用 | 删除用例在同一 state mutation 内移除该 Task 的全部入边 / 出边，再标记 Deleted；查询与 lifecycle 同时跳过 Deleted |
| INV-6 | 每个 live Task 的 `batch` 必须引用已存在 Batch；没有 active Batch 时不可创建 Task | `TaskAccess::create_task` 返回 `NoActiveBatch`；restore 校验所有 batch 引用 |
| INV-7 | 最多一个 Batch 为 Active，`current_batch` 必须精确指向它；没有 Active 时为 `None` | create / pause / resume / archive-by-id 在同一次 state mutation 中校验并更新 |
| INV-8 | Archived 是 Batch 终态；Paused 只能由 Active 进入，且只能在无其他 Active 时 resume | `pause_batch` / `resume_batch` / `archive_batch` 的 typed transition 守护 |
| INV-9 | 被未完成前置任务阻塞的 Task 不可进入 InProgress | `TaskAccess::transition(id, InProgress)` 在持有同一 store 写锁的一次 mutation 内读取依赖状态并迁移；失败返回 `TaskBlocked`，state 与事件均不变 |

### 1.3 聚合方法

Task 聚合只通过方法修改状态，外部不可直接写字段：

| 方法 | 语义 | 不变量 |
|---|---|---|
| `transition_to(new_status)` | Pending / InProgress / Completed 之间的状态迁移；`new_status=Deleted` **MUST** 拒绝 | INV-2：Deleted 只能经 store-backed delete 用例进入 |
| `set_priority(p)` | 更新优先级 | — |
| `add_tag(t)` / `remove_tag(t)` | 标签管理 | — |
| `mark_deleted()` | 在图边已清理后标记 Deleted | INV-2 / INV-5；只由 `TaskAccess::delete` 的 store-backed 用例调用 |

> **Decision**：依赖图操作跨越多个 Task，归 `TaskStoreState` 的领域服务方法所有；它在一次 mutation 中完成环检测及 `blocked_by` / `blocks` 双向更新。单个 Task 聚合方法 **NEVER** 接收 store 引用，也不独自维护跨聚合不变量。`transition_to()` 只守护单 Task 状态迁移矩阵；公开 `TaskAccess::transition(id, InProgress)` 还必须由 store-backed 命令在同一写锁 / mutation 内检查实时依赖状态并调用它，不能以先调用 `is_blocked` 再 transition 的 TOCTOU 流程代替。

## 2. TaskStatus 状态机

### 2.1 状态定义

```text
TaskStatus = Pending | InProgress | Completed | Deleted
```

### 2.2 迁移矩阵

| From → To | Pending | InProgress | Completed | Deleted |
|---|---|---|---|---|
| **Pending** | — | ✅ | ✅ | 仅 `TaskAccess::delete` |
| **InProgress** | ✅ | — | ✅ | 仅 `TaskAccess::delete` |
| **Completed** | ❌ | ❌ | — | 仅 `TaskAccess::delete` |
| **Deleted** | ❌ | ❌ | ❌ | — |

### 2.3 语义说明

- **Pending → InProgress**：Agent 开始执行该任务。首次迁移时设置 `started_at`；后续 `InProgress → Pending → InProgress` 保留首次开始时间，不覆盖。`TaskAccess::transition` **MUST** 在持有同一 store 写锁的一次 mutation 内，以当前权威依赖图检查 blocked；若任一 live 前置 Task 未完成则返回 `TaskCommandError::TaskBlocked { id, blocked_by }`，不修改 Task、不更新时间戳且不产生事件。
- **InProgress → Completed**：任务完成，设置 `completed_at`；`started_at` 保留首次开始时间。
- **InProgress → Pending**：任务退回待办（如发现前置条件不满足），保留 `started_at`，不设置 `completed_at`。
- **任意活状态 → Deleted**：**NEVER** 通过 `transition_to`；只能调用 `TaskAccess::delete`，在同一 state mutation 内先清理全部依赖边再标记；不修改执行时间。
- **Pending → Completed**：直接完成（如任务被判定无需执行），同一迁移时间同时写入 `started_at` 与 `completed_at`，因此执行耗时为 0。
- **→ Deleted**：软删除，从状态机和依赖图中摘除。**Deleted 不可逆**。
- **Deleted 是软删除标记，不是状态机正轨状态**：Deleted 的 Task 不参与 `is_blocked` 或 lifecycle 检测；tombstone 只在当前进程的 live state 中保留，snapshot 会过滤它，**NEVER** 把它描述为 durable audit 记录。若未来需要审计，必须另由 Audit BC 接收并持久化版本化 `TaskDeleted` fact。

### 2.4 领域事件

状态迁移产生领域事件，供 TUI 投影和日志消费：

| 事件 | 触发 |
|---|---|
| `TaskCreated` | 新建 Task |
| `TaskStatusChanged { from, to }` | 状态迁移 |
| `TaskDependencyAdded { task_id, blocked_by_id }` | 添加依赖 |
| `TaskDependencyRemoved { task_id, blocked_by_id }` | 移除依赖 |
| `TaskDeleted { task_id }` | 清理依赖边并在 live state 标记 Deleted；仅领域事件，不承诺 durable audit |

> **Decision**：这些事件不是只能在 Task 内部观察的说明性对象。每个成功 mutation command 在持有同一 store lock 时完成状态修改并按确定顺序生成对应 `TaskEvent`，再经公开 `TaskCommandResult<T> { value, events }` 与更新后的 read model 一并返回；失败返回 error 且没有事件。Runtime 必须消费该原子结果，经 event projection ACL 把 `TaskEvent` 映射为 SDK `ChatEvent`，再由 TUI 投影；Task 事件是 TUI Task 状态的唯一事实来源，TUI **NEVER** 自行推导。查询不产生事件并继续返回纯值。内部 event bus 只有出现多个独立进程内消费者时才另行评估。

## 3. 依赖图不变量

### 3.1 DAG 约束

`blocked_by` 定义了一个有向无环图（DAG）：

- 边 `A → B`（A.blocked_by 包含 B）表示"A 被 B 阻塞"，即 B 完成前 A 不能开始。
- 添加边前必须检测是否会产生环。
- 自环（A.blocked_by 包含 A）等价于环，直接拒绝。

### 3.2 环检测算法

使用 DFS 检测：从被依赖项出发，沿 `blocked_by` 链向下遍历，若回到起点则存在环。

```
fn would_create_cycle(task_id, blocked_by_id) -> bool:
  if task_id == blocked_by_id: return true   // 自环
  visited = {}
  stack = [blocked_by_id]
  while stack not empty:
    current = stack.pop()
    if current == task_id: return true       // 回到起点 → 环
    if current in visited: continue
    visited.add(current)
    if let Some(t) = store.get(current):
      for dep in t.blocked_by:
        stack.push(dep)
  return false
```

### 3.3 is_blocked 判定

一个 Task 被阻塞当且仅当 `TaskStoreState` 中该 `TaskId` 当前实体的 `blocked_by` 引用了至少一个未完成（非 Completed 且非 Deleted）的 live Task；查询 **MUST** 只接受 `TaskId` 并在同一权威 state snapshot 内完成，**NEVER** 接受调用方持有的可能陈旧 `Task` read model：

```
fn is_blocked(task_id) -> bool:
  task = store.get(task_id) or return false
  for id in task.blocked_by:
    if let Some(t) = store.get(id):
      if t.status != Completed && t.status != Deleted:
        return true
  return false
```

### 3.4 反向投影 blocks

`blocks` 是 `blocked_by` 的反向投影：

- 当 `A.add_dependency(B)` 时：`A.blocked_by` 加入 `B`，`B.blocks` 加入 `A`。
- 当 `A.remove_dependency(B)` 时：`A.blocked_by` 移除 `B`，`B.blocks` 移除 `A`。
- `blocks` **NEVER** 被独立写入或修改。

> **Decision**：`blocks` 的存在是为了支持反向查询（"哪些任务依赖我"），避免每次查询都扫描全量。维护成本是 add / remove 时多一次跨聚合写入。

## 4. Batch 实体与生命周期服务

### 4.1 定位

Batch 是 Task-owned 有标识实体，存放在唯一 `TaskStoreState` 权威状态槽中：它有稳定 `BatchId`、状态与持久化生命周期，但 **不是独立聚合根**，`TaskStoreState` 也只是跨 Task / Batch 一致性用例的单一状态持有者。围绕 Batch 的归档 / 中断 / stale 检测函数才是纯领域服务。二者共同承担：

1. 将同一轮对话产生的 Task 归入同一 Batch（`batch: BatchId`）。
2. 管理 Batch 的生命周期状态（Active / Paused / Archived）。
3. 提供 lifecycle 检测函数（纯函数，无 I/O）。

### 4.2 BatchStatus

```text
BatchStatus = Active | Paused | Archived
```

| 状态 | 语义 |
|---|---|
| **Active** | 当前正在工作的批次 |
| **Paused** | 被用户中断，可恢复 |
| **Archived** | 已完成或废弃，归档 |

### 4.3 Batch 迁移矩阵

| From → To | Active | Paused | Archived |
|---|---|---|---|
| **Active** | — | `pause_batch(id)` | `archive_batch(id)` |
| **Paused** | `resume_batch(id)`；要求当前无其他 Active | — | `archive_batch(id)` |
| **Archived** | ❌ | ❌ | 幂等返回当前实体 |

`pause_batch` 在同一次 mutation 中把目标改为 Paused 并清空 `current_batch`；`resume_batch` 在确认没有其他 Active 后把目标改为 Active 并设置 `current_batch=Some(id)`；`archive_batch` 接受 Active / Paused 的明确 `BatchId`，必要时清空 `current_batch`。因此 Paused 是公开命令可达且可恢复的真实状态，**NEVER** 只是 serde 枚举中的死分支。

### 4.4 Batch 字段

| 字段 | 类型 | 说明 |
|---|---|---|
| `id` | `BatchId` | Task-owned 批次标识 |
| `summary` | `Option<String>` | 用户请求摘要；新建命令要求非空并写 `Some`，`None` 只用于兼容旧 snapshot |
| `status` | `BatchStatus` | 生命周期状态 |
| `created_at` | `u64` | 创建时间戳 |
| `last_active_turn` | `u64` | 最后活跃 turn |
| `silence_turns` | `u64` | 静默 turn 计数 |

Batch **没有** `description` 字段。新建只接收 typed `BatchCreateSpec { summary }`；Tool 层若同时有 subject / summary，**MUST** 在 ACL 中归一化成一个非空 summary，**NEVER** 把未存储的 description 参数发布到 Task OHS。

### 4.5 Lifecycle 检测函数

三个纯函数，输入 Task 与 Batch 快照，输出检测结果：

| 函数 | 场景 | 输出 |
|---|---|---|
| `detect_batch_all_completed` | 上一批次全部完成 → 应归档 | `Option<BatchId>` |
| `detect_interrupted_batch` | 新话题打断旧批次 → 旧批次有未完成任务 | `Option<InterruptedBatchInfo>` |
| `detect_stale_batches` | 批次静默过久 → 陈旧 | `Vec<StaleBatchInfo>` |

> **Decision**：检测函数是纯函数，不产生副作用。调用方根据检测结果，以明确 `BatchId` 调用 `pause_batch` / `archive_batch`；恢复由 `resume_batch` 执行。状态变化与 `current_batch` 更新仍由 TaskAccess 实现原子守护。

## 5. TaskSnapshot

### 5.1 结构

TaskSnapshot 是 Task 聚合群的可持久化快照，用于 Session 落盘与恢复：

| 字段 | 类型 | 说明 |
|---|---|---|
| `tasks` | `Vec<PersistedTask>` | 所有非 Deleted Task 的持久化 DTO；只含权威字段，**NEVER** 含派生 `blocks` |
| `next_task_id` | `TaskId` | 下一个待分配 Task ID；legacy wire 的 `next_id` 只由 Task-owned codec 升级 |
| `next_batch_id` | `BatchId` | 下一个待分配 Batch ID；legacy 缺失时由 codec 以最大已存 BatchId + 1 派生 |
| `current_batch` | `Option<BatchId>` | 唯一 Active Batch；没有 Active 时为 `None`，**NEVER** 使用 `0` 哨兵值 |
| `batches` | `Vec<Batch>` | 所有 Batch 元数据 |

### 5.2 快照边界

- 快照收集时**过滤 Deleted 状态**的 Task（不落盘已删除任务）；`TaskAccess::delete` 已先原子移除其全部依赖边，因此任何合法 live state 收集出的快照 **MUST** 不含 dangling reference，并能被同版本 `TaskPersist::prepare_restore` 接受。
- `PersistedTask` **MUST** 显式列出 `id / subject / description / status / ... / blocked_by` 等权威字段，并省略 `blocks`；`TaskSnapshot` **NEVER** 直接 serde 运行时 `Task`。restore 只从已验证的 `blocked_by` 构造全部反向 `blocks`，旧 wire 若携 `blocks` 只能由兼容 ACL 丢弃。
- Context Management 先取得 exclusive session-switch lease，再调用 `prepare_restore` 完整校验并构造 opaque token；同一 lease 阻止 Task mutation 发生在 token 生成与 commit 之间。`prepare_restore` **MUST** 显式拒绝任何 `PersistedTask.status == Deleted`，返回 `PersistedDeletedTask { id }`；不能依赖本版本 writer 的 collect 过滤来信任外部/legacy snapshot。`commit_restore` 随后无失败地**全量替换**内存状态（清空后写入）。因此 restore 后 live state **NEVER** 含 Deleted Task，**NEVER** “复活”已删除任务。
- 快照内嵌 Session 落盘，经 `TaskPersist` 收集，不是 Task BC 自己驱动持久化。

### 5.3 跨 BC 快照组装

Session 落盘时，Context Management 经 `TaskPersist::collect_snapshot()` 获取 `TaskSnapshot`，将其嵌入 Session 持久化 DTO。恢复时先取得 exclusive session-switch lease，再依序准备 Project → Config → Memory → Task：Project 验证 identity，Config 构造目标 snapshot，Memory 以 candidate config eager-open，最后 `TaskPersist::prepare_restore(snapshot)` 取得不修改 live state 的 token。四个 participant 全部 prepare 成功后，才在**同一 lease**内进入无失败 commit；任一 prepare 失败时四者 live state 全旧。Runtime / Tool 只获得同一 backing 的 `TaskAccess` view。

这是 Context Map §8 定义的 ACL 位置之一：跨 BC 快照组装经端口，不共享内部结构。

## 6. 标识策略

### 6.1 v0.1.0 决定

`TaskId(u64)` 与 `BatchId(u64)` 是 Task-owned 强类型；新 wire 使用十进制数字字符串（`"1"`、`"2"`、…），**NEVER** 以裸 `String` / `u64` 穿过 typed Task OHS。ID 在单 Session 内单调递增，由 `TaskStoreState.next_task_id` / `next_batch_id` 维护并随 snapshot 一起替换，**NEVER** 从 live state 继续旧计数器或重用已分配 ID。

wire 格式解析与 typed state 校验是两个阶段：Task-owned codec 先把 raw JSON 的全字符串新格式或全数字 legacy 格式转换为 `TaskSnapshot`，拒绝 invalid / mixed format；`TaskPersist::prepare_restore(&TaskSnapshot)` 接收的已经是强类型值，只校验 ID 唯一、引用、环与两个 next ID 严格大于各自已存最大值，**NEVER** 再声称“解析”ID。

UUIDv7 不属于 v0.1.0 Target。未来若改变标识格式，**MUST** 以独立 RFC 定义 wire version、旧 Session 升级、排序语义与 mixed-format 禁止规则；在此之前 **NEVER** 同时生成两种格式。

## 7. 相关文档

- Task 端口与 Published Language：[02-ports-and-published-language.md](02-ports-and-published-language.md)
- 模块入口：[README.md](README.md)
- 统一语言：[../../01-system/02-ubiquitous-language.md](../../01-system/02-ubiquitous-language.md) §6
- Context Map：[../../01-system/03-context-map.md](../../01-system/03-context-map.md) §6 / §7 / §8 / §10
- Current → Target 迁移责任：[../../03-engineering/03-migration-governance.md](../../03-engineering/03-migration-governance.md)

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-12 | 初稿：Task 聚合根、状态机迁移矩阵、依赖图不变量、Batch 与 lifecycle、TaskSnapshot | #791 |
| 2026-07-14 | TaskSnapshot 恢复改为 prepare / commit token，由 Context Management 与 Project 联合协调 | [#972](https://github.com/rushsinging/aemeath/issues/972) |
| 2026-07-14 | 固定 v0.1.0 的单 Session 单调数字 ID，移除未排期的 UUIDv7 Current → Future 混写 | [#972](https://github.com/rushsinging/aemeath/issues/972) |
| 2026-07-14 | 分离 Task-owned wire codec 与 typed prepare，并持久化 next_task_id / next_batch_id 防止恢复后 ID 复用 | [#972](https://github.com/rushsinging/aemeath/issues/972) |
| 2026-07-14 | 固化 typed/fallible create 与 Batch pause/resume/archive-by-id 迁移，使 Paused 可达并移除未持久化 description 参数 | [#972](https://github.com/rushsinging/aemeath/issues/972) |
| 2026-07-16 | 校验 #885 领域内核实现：强类型 ID、私有实体、typed create、严格局部状态机、事件、聚合骨架与 typed lifecycle 对齐；DAG、端口、snapshot/restore、ACL 与 legacy 退役仍按 #886–#891 承接 | [#885](https://github.com/rushsinging/aemeath/issues/885) |
| 2026-07-16 | 从 Task 实体移除 `owner`：Agent 分配属于 Runtime-owned 执行绑定，legacy Tool/Storage 字段由 #889/#891 迁移退役 | [#885](https://github.com/rushsinging/aemeath/issues/885) |
| 2026-07-16 | 增加 Task-owned `started_at` / `completed_at` 执行时间事实，锁定首次开始、重入保留与 Pending 直接完成语义；snapshot/codec 由 #888/#890 承接 | [#885](https://github.com/rushsinging/aemeath/issues/885) |
