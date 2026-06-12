<!-- Migrated from: docs/feature/archived/024-task-list-windowed-display.md -->
# Feature #24: Spinner 下方 task list 限量显示（最多 7 条）

- **完成日期**：2026-05
- **归档日期**：2026-05-14
- **状态**：已确认完成

## 目标

task 多时显示过长挤占主输出。改为窗口化显示，总数封顶 7 条。

## 完成内容

- 窗口化显示：上一条 completed + 所有 in_progress + 后续 pending，总数封顶 7 条
- 其余以 `… +N more` 折行提示
- 摘要行 `Tasks: x/y` 仍反映全量进度
