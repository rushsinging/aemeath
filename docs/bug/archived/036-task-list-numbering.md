# Bug #36: TaskListCreate 后新任务编号未从 1 开始

- **发现日期**：2026-05
- **归档日期**：2026-05-14
- **状态**：已确认修复
- **优先级**：中

## 症状

调用 `TaskListCreate` 创建新的 task list 后，后续通过 `TaskCreate` 新增任务时，用户可见编号没有从 1 重新开始，而是继续沿用之前 task list / 全局任务的递增编号（例如新列表第一条显示为 `#6`）。

## 根因

TaskStore 使用全局递增 task id 作为用户可见编号，TaskListCreate 只创建 batch/list 边界，没有为每个 task list 维护局部序号或显示编号映射。

## 修复（2026-05-11）

1. TUI task list 渲染改用 batch 内局部显示编号，不再直接展示全局 task id；同一 batch 内按全局 id 稳定映射为 `#1/#2/...`。
2. `TaskStore::list_current_batch()` 改为只选择 Active/Paused batch，已归档 batch 不再因 task id 最大而被误显示。
3. 新增回归测试覆盖第二个 task list 从 `#1` 开始显示、已归档 batch 不再出现在当前 batch 列表。

## 涉及路径

- `aemeath-core/src/task.rs`
- `aemeath-tools/src/task_list_create.rs`
- `aemeath-tools/src/task_create.rs`
- `aemeath-tools/src/task_list.rs`
- TUI task list 渲染相关路径
