<!-- Migrated from: docs/feature/archived/064-tool-result-preview.md -->
# Feature #64：TUI tool call result 子块展示 output 内容预览

**状态**：✅ 已完成（2026-05-30 用户确认）

**优先级**：中

## 背景

#63 之后，tool call result 子块仍只展示 `✓ X completed` 摘要，看不到工具实际输出内容；与 bug #86/#87 一同要求"既不刷屏又能看到关键预览"。

## 解决方案

- result 子块从纯 `✓ X completed` 摘要改为展示工具实际 output 的**前 N 行预览**（受各工具 `result_max_lines` 截断，默认 5 行 + `N lines omitted`）。
- 承接 #87/#86：完整内容不刷屏由渲染层 `format_result_lines` 截断 + `bind_tool` 修复保证 id 不丢共同保证。
- **改动**：
  - 嵌入路径 `result_summary` 改携带实际 `call.result`（删除 assembler 的 `tool_result_summary`）。
  - `format_result_lines` 对 `max_lines==0` 工具（AskUserQuestion/TaskListComplete，答案已 echo）整体不渲染。

## 回归测试

- `output_tests` 嵌入预览断言重写。
- `test_render_tool_result_max_lines_zero_renders_nothing`。
- `state::tests` 端到端 Grep 预览。

## 相关提交

- `f2afc59` feat(tui): tool call result 子块展示 output 前 N 行预览 (#64, refs #87 #86)

## 验证

`cargo test -p cli` 全通过。2026-05-30 用户确认 feature #64 已完成。
