<!-- Migrated from: docs/feature/archived/013-task-list-below-spinner.md -->
# #13 Task list 显示在 spinner 下方

**归档日期**：2026-05-04

**确认结果**：用户确认完成

**目标**：调整 TUI 临时区域渲染顺序，让 task list 显示在 spinner 下方，作为靠近 input area 的稳定任务面板，同时保证 spinner 紧贴输出流并始终可见。

**实现**：
- 临时区域顺序调整为 `queued messages → spinner → task status lines`。
- task list 沉到底部，spinner 不再被 task 条目数量变化上下推动。
- 临时区域空间不足时优先保留 spinner，task list 可折叠/截断，避免重现 spinner 被挤出的问题。

**涉及文件**：
- `aemeath-cli/src/tui/output_area/mod.rs`
- `aemeath-cli/src/tui/output_area/spinner.rs`
- `aemeath-cli/src/tui/app/render.rs`

**关联**：
- Bug #24：spinner 偶尔消失，归档时已确认未退化。
- Bug #25：/clear 未清空 status line，归档时已确认不影响 reset 路径。
