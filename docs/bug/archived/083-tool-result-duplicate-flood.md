# Bug #83：TUI 渲染 tool call 同时输出 summary 和完整内容，重复刷屏

| 字段 | 值 |
|------|-----|
| 优先级 | 中 |
| 发现日期 | 2026-05 |
| 归档日期 | 2026-05-30 |
| 状态 | 已确认修复 |
| 根因类别 | TUI 渲染 / OrphanToolResult 提升 |

## 症状

所有工具（Read/Grep/Bash/Edit 等）的 tool result 在 TUI 工具块结果摘要区展示完整内容，导致长输出重复刷屏。Read 工具先显示 `✓ Read(...)` + `Read <path>` 详情行，紧接着又在工具块内把整个文件内容原样输出一遍。

## 根因

1. `ToolDisplay` trait 已定义 `format_result_summary()`，但 #58 新管线中的 `OutputViewAssembler::find_tool_view()` 曾未调用它。
2. `find_tool_view()` 直接把完整 tool result 塞入 `ToolCallBlockView.result_summary`，该字段会渲染在工具块结果摘要区。
3. ToolResult 事件可能先于正式 ToolCall 绑定到达；旧逻辑先创建 `OrphanToolResult`，后续 ToolCall 绑定时没有按 id 提升该 orphan result，导致完整结果继续作为块外 `DiagnosticNotice` 渲染。

## 修复

- `find_tool_view()` 改用 `ToolDisplay::format_result_summary()` 生成短摘要；未注册 display 时回退为 `✓ <tool> completed` / `✗ <tool> failed`。
- 完整 tool result 仍保存在 conversation/tool call 中供模型上下文使用，但不直接作为 TUI summary 输出。
- ToolCall 绑定时如果发现同 id 的 orphan result，会先移除 orphan block、完成 active tool，再插入可被 assembler 去重的 `ToolResult` block，避免完整结果泄漏到工具块外。

## 回归测试

1. `test_output_assembler_summarizes_embedded_tool_result_without_full_output`
2. `test_output_assembler_uses_error_summary_for_failed_tool_result`
3. ToolCall 绑定时 orphan result 提升的测试覆盖

## 相关提交

- `bb27783` fix(tui): 修复 tool result 块外泄漏 (refs #83)
- `375b29d` test(tui): 更新工具结果摘要渲染期望 (refs #83)
- `17e6b1a` fix(tui): 修复工具结果重复刷屏 (refs #83)

## 验证

2026-05-30 用户确认 bug #83 已修复。
