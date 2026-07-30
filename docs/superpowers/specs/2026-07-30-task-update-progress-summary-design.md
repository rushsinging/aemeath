# TaskUpdate 进度摘要与独立 Task Reminder 退役设计

> 对应 Issue: https://github.com/rushsinging/aemeath/issues/1456
> Milestone: v0.1.0 — Context Engineering + 架构重构

## 目标

移除独立的周期性 task reminder 注入，改由 `TaskUpdate(status)` 成功返回当前 task list 的结构化进度摘要，降低模型无效调用 `TaskListGet` 的概率，并统一 Task Domain 的列表生命周期语义。

## 背景与根因

现有 Context 在每次 Provider invocation 中读取 active task list，并把未完成数量渲染为 `<task-reminder>`，追加到最后一条真实 user message。文案同时提示模型在相关时调用 `TaskListGet`。由于 reminder 与模型调用绑定，而不是与任务状态变更绑定，旧 task list 会在后续每轮重复出现；只有统计数量又不足以决定下一步，模型因此容易反复查询完整任务列表。

现有 Task 模型已经持有 `completed_at`，且 Task Domain 已拥有依赖图、Batch 状态和原子 revision 提交能力。因此根因级方案是让状态变更结果携带一次权威进度投影，而不是在 Context 和 Tool Adapter 中维护第二套提醒逻辑。

## 设计决策

### 1. Task Domain 负责原子进度投影

Task Domain 新增结构化 `TaskProgressSnapshot`，由状态变更后的同一份领域状态生成。Tool Adapter 不重新遍历任务或推导 Batch 生命周期，只渲染该投影。

投影至少包含：

- 当前 task list：ID、summary、Batch 状态；
- 本次更新的任务：ID、subject、status；
- `recently_completed`：按 `completed_at` 倒序，最多 2 个，显示 ID、subject；
- `in_progress`：当前 Batch 内全部进行中任务，显示 ID、subject；
- `ready`：当前 Batch 内 pending 且没有未完成依赖的任务，最多 2 个，显示 ID、subject；
- `omitted`：被截断的 ready 数量和被阻塞 pending 数量；
- 生命周期事件：是否自动关闭、是否自动重新打开、是否发生 active list 冲突。

`completed_at` 已存在于 Task 持久化模型。任务从 Completed 回到 Pending/InProgress 时清除旧完成时间，再次完成时写入新的状态变更时间，因此最近完成排序不需要新增 schema 字段。

### 2. 仅 status 更新附带摘要

`TaskUpdate` 的 `key=status` 成功后返回进度摘要；`subject`、`description`、`priority` 更新继续返回现有短结果，避免无关字段修改造成大段上下文噪声。

摘要使用结构化 typed result 和人类可读文本共用同一领域投影，避免 JSON 与文本分别计算出不同结果。

### 3. Batch 生命周期与状态变更同一原子操作

- 状态更新后，如果当前 Batch 没有 Pending 或 InProgress 任务，Task Domain 自动将 Batch 归档并清除 `current_batch`。
- 返回明确通知 task list 已自动关闭，后续不会再产生提醒。
- 自动关闭的 Batch 中任务改回 Pending/InProgress 时：
  - 没有其他 Active Batch：在同一领域命令中重新激活该 Batch，并返回重新打开通知；
  - 已有其他 Active Batch：拒绝整个操作，任务状态、Batch 状态和 revision 均不发生变化，并返回 active list 冲突错误。
- `InProgress` 仍必须通过现有依赖检查；存在未完成依赖时拒绝迁移。
- `Deleted` 状态导致 Batch 没有未完成任务时，也执行同样的自动关闭规则。
- `TaskListComplete` 保留 Published Language 兼容性；已经自动关闭时返回幂等成功。

### 4. 删除独立 reminder 双轨

删除以下生产路径和对应过期测试：

- Context 的 `TaskReminderSnapshot`、`InvocationReminder` 和 ContextRequest/ContextWindow 字段；
- Runtime 的 `TaskReminderState`、Task 工具调用观察和相关装配字段；
- Provider invocation 中追加 `<task-reminder>` 的逻辑；
- `TaskListGet` / `TaskUpdate` 文案中“完成后调用 TaskListGet”的强制性指引。

`TaskListGet` 改为仅在最近一次 `TaskUpdate(status)` 摘要不足以决定下一步，或用户明确要求完整列表时调用。模型不得仅因为存在 Pending/InProgress 任务就调用该工具。

### 5. 兼容性

Task snapshot 中已有的 `completed_at` 继续沿用；旧 task snapshot 和 session 数据必须保持可读。移除的是 Context invocation-only 装饰，不改变 canonical session 消息、SDK/TUI 事件或历史 JSON 的既有语义。

## 数据流

```text
TaskUpdate(status)
    -> Task Domain 原子状态迁移
    -> Batch 自动关闭/重新激活/冲突校验
    -> TaskProgressSnapshot
    -> Tool Adapter 双语文本 + typed JSON
    -> Tool result 注入下一轮模型上下文
```

不存在从 Context 到 Provider 的独立 task reminder 数据流。

## 测试策略

- **Task Domain L1/L2**：状态摘要的排序、截断、ready/block 判断、全部完成自动关闭、关闭后重新激活和 active list 冲突原子失败。
- **Task Adapter L3**：status-only 返回、结构化字段完整性、双语文案、错误时无部分状态提交。
- **Context/Runtime/Provider L2/L3**：ContextRequest、ContextWindow 和 invocation 映射不再包含或追加 task reminder；旧消息保持不变。
- **跨层 L4**：从 TaskUpdate status 变更到模型可见 tool result 的组合场景，覆盖进行中任务全量返回、ready 截断和自动关闭通知。
- 所有新增核心逻辑遵循 TDD，先提交失败测试，再实现；执行定向测试、相关 crate 测试、`cargo fmt --check`、clippy 和架构守卫。

## 非目标

- 不新增固定轮次或事件后的 Context reminder 兜底。
- 不改变 TaskListGet 的完整列表查询能力。
- 不改变 TUI task list 的展示模型，除非编译契约要求同步字段。
- 不自动切换或暂停另一个 active Batch；冲突必须显式失败。
