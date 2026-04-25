# Tool call 后 Thinking 状态栏卡在 "Calling xxx..."

## 症状
模型完成 tool call 后重新开始 thinking 时，状态栏仍然显示 "Calling xxx..."，thinking 文本也不显示在 output area 中。

## 根因
`UiEvent::Thinking` 处理中，当 `tool_call_active == true` 时直接跳过（`if !self.tool_call_active { ... }`），导致：
1. thinking 文本被丢弃
2. `tool_call_active` 未重置
3. 状态栏的 "Calling xxx..." 未清除

`ToolResult` 事件虽然设置了 `tool_call_active = false`，但多轮 tool call 场景下（模型在 tool call 之间穿插 thinking），状态栏显示不正确。

## 修复
- `Thinking` 事件：无论 `tool_call_active` 状态，都正常显示 thinking 文本。若 `tool_call_active` 为 true，同步重置并更新状态栏为 "Thinking..."
- `Text` 事件：同理，若 `tool_call_active` 为 true，同步重置并更新状态栏为 "Generating..."

## 回归测试
- 单轮 tool call 后 thinking 正常显示
- 多轮 tool call 间穿插 thinking 时状态栏正确切换
- tool call 后直接输出文本（无 thinking）时状态栏正确

## 关联
- 涉及路径：`aemeath-cli/src/tui/app/event_handler.rs`

---
**发现日期**：2026-04-25
**已归档**：已修复后从 `active.md` 移出，文件放入 `archived/` 目录
