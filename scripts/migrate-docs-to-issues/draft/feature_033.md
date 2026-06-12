<!-- Migrated from: docs/feature/archived/033-task-list-display-optimization.md -->
# Feature #33: 优化 TaskListCreate / TaskListComplete 工具调用显示

- **完成日期**：2026-05-14
- **归档日期**：2026-05-14
- **状态**：已确认完成

## 目标

降低 task batch 管理工具在 TUI 输出中的噪声，让用户只看到本次 task list 的主题、摘要和完成动作，不再显示原始 JSON 与成功结果噪声。

## 完成内容

1. 新增 `TaskListCreateDisplay`：header 显示为 `TaskListCreate: <subject>`，详情只显示 `summary`，不显示完整 JSON。
2. 保持 `TaskListCreate` 现有显示不变；`TaskListComplete` 成功后只显示 `✓ TaskListComplete`，无参数详情、无成功结果正文、无额外空行。
3. `TaskListCreate` 成功结果正文继续静默，避免重复显示 `created` 噪声；`TaskListComplete` 仅在错误时显示失败摘要。
4. 新增/更新回归测试覆盖 TaskListCreate 既有显示和 TaskListComplete 单行成功输出。

## 涉及路径

- `aemeath-cli/src/tui/output_area/tool_display/task_impls.rs`
- `aemeath-cli/src/tui/output_area/tool_display/results.rs`
- `aemeath-cli/src/tui/output_area/tool_display.rs`
