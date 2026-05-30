# Bug #87：TUI tool call 显示完整 tool result 内容且不受 max output 限制，result 渲染格式错误

| 字段 | 值 |
|------|-----|
| 优先级 | 高 |
| 发现日期 | 2026-05 |
| 归档日期 | 2026-05-30 |
| 状态 | 已确认修复 |
| 根因类别 | TUI 渲染 / Orphan tool result 截断与摘要 |

## 症状

tool call result 在 TUI 中渲染时存在两类问题：

1. **格式错误**：直接展示工具返回的原始 diff 内容或 Read 完整输出，而非格式化的摘要视图。
2. **不受最大行数限制**：大文件操作的完整内容全部刷屏，看起来像 LLM 正文从 tool call result 中刷出。

## 根因

- 嵌入 ToolResult 路径需要只展示 `ToolDisplay::format_result_summary()` 生成的短摘要。
- 非嵌入/Orphan ToolResult 路径若直接透传 `output`，会被 `DiagnosticNotice` 逐行渲染完整内容。
- `OrphanToolResult` 路径（结果早于 ToolCall 绑定且未被提升）仍走 `summarize_orphan_result` 截断透传原始带行号 `output`，并以 `Warning`（橙）色整段刷出——表现为"正文刷屏 + 颜色不对"。

## 修复

1. ToolResult 子块只展示短摘要（例如 `✓ Read completed`），完整 Read 内容不进入渲染文本。
2. assistant 正文保持独立 `AssistantMessage` block，不混入 ToolResult。
3. 非嵌入与 Orphan ToolResult 均使用工具 display 摘要（不再透传/截断原始 output）。
4. `ConversationBlock::OrphanToolResult` 新增 `tool_name` 字段，`observe_tool_result` push 时写入；assembler 的 orphan 臂改走 `summarize_non_embedded_result(Some(tool_name), ..)`，与非嵌入路径统一（DRY）；颜色随 Success/Error 而非 Warning。删除冗余的 `summarize_orphan_result`。

## 回归测试

1. `test_output_assembler_summarizes_embedded_tool_result_without_full_output`
2. `test_output_assembler_keeps_assistant_text_outside_read_result`
3. `test_non_embedded_tool_result_uses_summary`
4. `test_orphan_read_result_shows_summary_not_full_content`
5. `test_orphan_tool_result_shows_summary_not_raw_output`

## 相关提交

- `929fbdb` fix(tui): 防止 Read result 刷出正文 (refs #87)
- `c684e40` fix(tui): tool result 颜色跟随 tool call 状态而非硬编码 TEXT_DIM (refs #87)
- `997cfbb` fix(tui): 非嵌入 ToolResult/OrphanToolResult 截断与摘要化 (refs #87)
- `9e6dc88` fix(tui): orphan tool result 走摘要 + Read 头部去重复路径 (refs #87 #88)
- `07eefb1` fix(tui): bind_tool 只绑未绑定占位 + 非嵌入结果永不刷原始 output (refs #87 #86)
- `f2afc59` feat(tui): tool call result 子块展示 output 前 N 行预览 (#64, refs #87 #86)
- 拆分/重构相关：`ec7612a`、`e5946ab`、`3fbcc88`、`4fa746e`

## 验证

2026-05-30 用户确认 bug #87 已修复。
