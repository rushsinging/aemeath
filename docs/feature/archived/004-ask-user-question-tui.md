# #4 AskUserQuestion TUI 美化

**归档日期**：2026-05-06

**确认结果**：用户确认完成

## 目标

当 LLM 调用 AskUserQuestion tool call 时，TUI 中的确认界面需要美化，提升可读性和交互体验。

## 完成内容

- AskUserQuestion 已接入 TUI 交互流程。
- 通过 `UiEvent::AskUser` 进入 TUI 状态更新。
- `update.rs` 中维护 `ask_user_reply_tx`，支持用户在 TUI 中选择/输入答案并回传 tool call。
- 界面不再只是普通输出流里的无结构文本，而是进入专门的 ask-user 状态处理。

## 涉及文件

- `aemeath-cli/src/tui/app/update.rs`
- `aemeath-cli/src/tui/output_area/`

## 验证

用户已确认完成。
