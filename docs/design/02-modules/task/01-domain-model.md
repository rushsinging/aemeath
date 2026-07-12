# Task 领域模型

> 层级：02-modules / task（模块战术设计）
> 状态：Target（目标设计）｜Milestone：v0.1.0｜对应 Issue：#791（S2）

## 1. Task 聚合根

Task 是 Task Management BC 的聚合根，封装一个可追踪工作单元的全部状态与不变量。

### 1.1 字段

| 字段 | 类型 | 说明 |
|---|---|---|
| `id` | `String`（目标 `UUIDv7`） | 聚合标识，当前用自增数字字符串，目标迁移到 UUIDv7 |
| `subject` | `String` | 任务标题（简短一行） |
| `description` | `String` | 任务描述（多行） |
| `status` | `TaskStatus` | 状态机当前状态 |
| `active_form` | `Option<String>` | 进行时描述（spinner 显示用） |
| `owner` | `Option<String>` | 任务分配对象（agent 名称或标识） |
| `blocked_by` | `Vec<String>` | 依赖的 Task ID 列表 |
| `blocks` | `Vec<String>` | 被 此 Task 阻塞的 Task ID 列表（反向投影） |
| `priority` | `TaskPriority` | 优先级（Low / Normal / High / Urgent） |
| `progress` | `u8` | 进度百分比（0–100） |
| `progress_message` | `Option<String>` | 进度状态消息 |
| `created_at` | `u64` | 创建时间戳（ms since epoch） |
| `updated_at` | `u64` | 最后更新时间戳 |
| `session_id` | `Option<String>` | 所属 Session |
| `tags` | `Vec<String>` | 分类标签 |
| `batch` | `u64` | 所属 Batch ID |

### 1.2 不变量

| # | 不变量 | 守护方式 |
|---|---|---|
| INV-1 | `id` 创建后不可变 | 聚合方法不接受 id 修改参数 |
| INV-2 | `status` 只能沿合法迁移路径变化 | 状态迁移经聚合方法 `transition_to()` 守护 |
| INV-3 | `blocked_by` 不成环 | 添加依赖前 DFS 环检测 |
| INV-4 | `blocks` = 全局 `blocked_by` 的反向投影 | 由聚合在 add / remove dependency 时同步维护，禁止独立写入 |
| INV-5 | `progress` ∈ [0, 100] | `set_progress` 方法 clamp |
| INV-6 | `Deleted` 状态的 Task 不参与依赖图计算 | `is_blocked` 和 lifecycle 检测跳过 Deleted |

### 1.3 聚合方法

Task 聚合只通过方法修改状态，外部不可直接写字段：

| 方法 | 语义 | 不变量 |
|---|---|---|
| `transition_to(new_status)` | 状态迁移 | INV-2：校验迁移路径合法 |
| `add_dependency(other_id, store)` | 添加 blocked_by 项 | INV-3：环检测；INV-4：同步更新对方 blocks |
| `remove_dependency(other_id, store)` | 移除 blocked_by 项 | INV-4：同步移除对方 blocks |
| `set_progress(pct, msg)` | 更新进度 | INV-5：clamp 0–100 |
| `set_priority(p)` | 更新优先级 | — |
| `add_tag(t)` / `remove_tag(t)` | 标签管理 | — |
| `soft_delete()` | 标记 Deleted | INV-2：Deleted 不可逆 |

> **Decision**：`add_dependency` / `remove_dependency` 需要访问其他 Task 以维护反向投影，因此通过 `TaskPort` 或传入 store 引用完成跨聚合操作。这是聚合间协作，不是共享状态。

## 2. TaskStatus 状态机

### 2.1 状态定义

```text
TaskStatus = Pending | InProgress | Completed | Deleted
```

### 2.2 迁移矩阵

| From → To | Pending | InProgress | Completed | Deleted |
|---|---|---|---|---|
| **Pending** | — | ✅ | ✅ | ✅ |
| **InProgress** | ✅ | — | ✅ | ✅ |
| **Completed** | ❌ | ❌ | — | ✅ |
| **Deleted** | ❌ | ❌ | ❌ | — |

### 2.3 语义说明

- **Pending → InProgress**：Agent 开始执行该任务。
- **InProgress → Completed**：任务完成。
- **InProgress → Pending**：任务退回待办（如发现前置条件不满足）。
- **Pending → Completed**：直接完成（如任务被判定无需执行）。
- **→ Deleted**：软删除，从状态机和依赖图中摘除。**Deleted 不可逆**。
- **Deleted 是软删除标记，不是状态机正轨状态**：Deleted 的 Task 不参与 `is_blocked` 计算、不参与 lifecycle 检测，但数据保留以便审计。

### 2.4 领域事件

状态迁移产生领域事件，供 TUI 投影和日志消费：

| 事件 | 触发 |
|---|---|
| `TaskCreated` | 新建 Task |
| `TaskStatusChanged { from, to }` | 状态迁移 |
| `TaskProgressUpdated { progress, message }` | 进度更新 |
| `TaskDependencyAdded { task_id, blocked_by_id }` | 添加依赖 |
| `TaskDependencyRemoved { task_id, blocked_by_id }` | 移除依赖 |
| `TaskDeleted` | 软删除 |

> **Decision**：领域事件目前以 SDK `ChatEvent` 变体传播到 TUI；未来可扩展为内部 event bus。事件是 TUI 状态投影的唯一来源，TUI 不自行推导 Task 状态。

## 3. 依赖图不变量

### 3.1 DAG 约束

`blocked_by` 定义了一个有向无环图（DAG）：

- 边 `A → B`（A.blocked_by 包含 B）表示"A 被 B 阻塞"，即 B 完成前 A 不能开始。
- 添加边前必须检测是否会产生环。
- 自环（A.blocked_by 包含 A）等价于环，直接拒绝。

### 3.2 环检测算法

使用 DFS 检测：从被依赖项出发，沿 `blocked_by` 链向下遍历，若回到起点则存在环。

```
fn would_create_cycle(task, blocked_by_id) -> bool:
  if task.id == blocked_by_id: return true   // 自环
  visited = {}
  stack = [blocked_by_id]
  while stack not empty:
    current = stack.pop()
    if current == task.id: return true       // 回到起点 → 环
    if current in visited: continue
    visited.add(current)
    if let Some(t) = store.get(current):
      for dep in t.blocked_by:
        stack.push(dep)
  return false
```

### 3.3 is_blocked 判定

一个 Task 被阻塞当且仅当其 `blocked_by` 中存在未完成（非 Completed 且非 Deleted）的 Task：

```
fn is_blocked(task) -> bool:
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

## 4. Batch 领域服务

### 4.1 定位

Batch **不是独立聚合根**，而是 Task 集合的分组与生命周期管理领域服务。它的职责：

1. 将同一轮对话产生的 Task 归入同一 Batch（`batch: u64`）。
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

### 4.3 Batch 字段

| 字段 | 类型 | 说明 |
|---|---|---|
| `id` | `u64` | 批次标识 |
| `summary` | `Option<String>` | 用户请求摘要 |
| `status` | `BatchStatus` | 生命周期状态 |
| `created_at` | `u64` | 创建时间戳 |
| `last_active_turn` | `u64` | 最后活跃 turn |
| `silence_turns` | `u64` | 静默 turn 计数 |

### 4.4 Lifecycle 检测函数

三个纯函数，输入 Task 与 Batch 快照，输出检测结果：

| 函数 | 场景 | 输出 |
|---|---|---|
| `detect_batch_all_completed` | 上一批次全部完成 → 应归档 | `Option<batch_id>` |
| `detect_interrupted_batch` | 新话题打断旧批次 → 旧批次有未完成任务 | `Option<InterruptedBatchInfo>` |
| `detect_stale_batches` | 批次静默过久 → 陈旧 | `Vec<StaleBatchInfo>` |

> **Decision**：检测函数是纯函数，不产生副作用。归档 / 暂停动作由调用方（Runtime 或 TaskPort 实现）根据检测结果执行。

## 5. TaskSnapshot

### 5.1 结构

TaskSnapshot 是 Task 聚合群的可持久化快照，用于 Session 落盘与恢复：

| 字段 | 类型 | 说明 |
|---|---|---|
| `tasks` | `Vec<Task>` | 所有非 Deleted 的 Task |
| `next_id` | `u64` | 下一个 Task ID |
| `current_batch` | `u64` | 当前 Batch ID |
| `batches` | `Vec<Batch>` | 所有 Batch 元数据 |

### 5.2 快照边界

- 快照收集时**过滤 Deleted 状态**的 Task（不落盘已删除任务）。
- 快照恢复时**全量替换**内存状态（清空后写入）。
- 快照内嵌 Session 落盘，经 `TaskPort` 收集，不是 Task BC 自己驱动持久化。

### 5.3 跨 BC 快照组装

Session 落盘时，Context Management 经 `TaskPort::collect_snapshot()` 获取 `TaskSnapshot`，将其嵌入 Session 持久化 DTO。恢复时 Context Management 经 `TaskPort::restore_snapshot(snapshot)` 分发回去。

这是 Context Map §8 定义的 ACL 位置之一：跨 BC 快照组装经端口，不共享内部结构。

## 6. 标识策略

### 6.1 当前状态

Task ID 使用自增数字字符串（`"1"`、`"2"`、…），由 TaskStore 维护 `next_id` 计数器。Batch ID 同理。

### 6.2 目标态

- Task ID 目标迁移到 `UUIDv7`（与统一语言 §8 `ID` 定义一致），消除自增计数器的状态管理和 ID 重用问题。
- 迁移时机：S5 Runtime 模块迁移阶段，与 TaskStore 重构同步进行。
- 迁移期间旧 ID 格式需兼容（已有 session 数据可恢复）。

> **Decision**：当前自增 ID 在单 Session 内是稳定的，不阻塞战术设计定稿。UUIDv7 迁移作为 S5 的独立子任务跟踪。

## 7. 相关文档

- Task 端口与 Published Language：[02-ports-and-published-language.md](02-ports-and-published-language.md)
- 模块入口：[README.md](README.md)
- 统一语言：[../../01-system/02-ubiquitous-language.md](../../01-system/02-ubiquitous-language.md) §6
- Context Map：[../../01-system/03-context-map.md](../../01-system/03-context-map.md) §6 / §7 / §8 / §10

## 修改历史

| 日期 | 变更 | 关联 |
|---|---|---|
| 2026-07-12 | 初稿：Task 聚合根、状态机迁移矩阵、依赖图不变量、Batch 领域服务、TaskSnapshot | #791 |
