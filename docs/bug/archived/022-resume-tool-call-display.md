# Bug #22 Resume 时部分 tool call 信息显示不全

**状态**：✅ 已修复，用户已确认
**发现日期**：2026-04
**确认日期**：2026-04-29
**优先级**：中
**修复 commit**：待提交

## 症状

通过 resume 恢复历史会话时，部分 tool call 的展示信息不如实时执行时完整。之前已专门优化过 TaskCreate / TaskUpdate 的实时显示，但 resume 历史渲染路径下仍可能只显示较简略的 tool call 信息，导致 subject、description、状态变更、结果摘要等关键信息缺失或不一致。

## 根因

resume 使用 `render_history_message()` 重放 `Message::content` 中的 `ToolUse` / `ToolResult`，实时执行则走 `UiEvent::ToolCall` / `ToolResult` → `push_tool_call()` / `push_tool_result_with_diff()`。

两条路径没有完全复用同一套 ToolDisplay 逻辑，尤其是 TaskCreate / TaskUpdate / TaskList 等专用 display 的 header、detail、result_max_lines、summary 格式可能在历史重放中丢失。

## 修复

- `render.rs`：`render_history_message` 改为接收 `subsequent_msg` 参数，通过 `tool_use_id` 将 Assistant 消息的 ToolUse 与下一条 User 消息的 ToolResult 配对。
- Assistant 消息中的 ToolUse 改为调用 `push_tool_call()` + `push_tool_result_with_diff()`，复用完整的 ToolDisplay 逻辑，包括 header、details、result_max_lines、summary 等。
- User 消息中的 ToolResult 由配对逻辑渲染，User 侧不再直接逐行输出 ToolResult。
- 调用处（`mod.rs`、`slash.rs`）改为窗口式迭代，传入下一条消息作为 `subsequent_msg`。

## 涉及文件

- `aemeath-cli/src/tui/app/render.rs`
- `aemeath-cli/src/tui/app/mod.rs`
- `aemeath-cli/src/tui/app/slash.rs`
- `aemeath-cli/src/tui/output_area/tool_display.rs`

## 验证

用户已确认修复。
