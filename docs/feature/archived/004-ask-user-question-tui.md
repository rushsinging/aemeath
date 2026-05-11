# #4 AskUserQuestion TUI 美化

**归档日期**：2026-05-11

**确认结果**：用户确认完成

## 目标

当 LLM 调用 AskUserQuestion tool call 时，TUI 中的确认界面需要美化，提升可读性和交互体验。

## 阶段一：基础交互接入（2026-05-06）

- AskUserQuestion 已接入 TUI 交互流程。
- 通过 `UiEvent::AskUser` 进入 TUI 状态更新。
- `update.rs` 中维护 `ask_user_reply_tx`，支持用户在 TUI 中选择/输入答案并回传 tool call。
- 界面不再只是普通输出流里的无结构文本，而是进入专门的 ask-user 状态处理。

## 阶段二：UI 视觉美化（2026-05-11）

commit `3cadce4`

1. 新增 `LineStyle::AskUser` 样式（亮黄色 + 粗体），在 `types.rs` 的 `to_style()` 中映射。
2. 重写 `push_ask_user()` 美化：
   - 顶部添加醒目标题行 `━━ 需要你的回答 ━━`（AskUser 样式）
   - 问题文本使用 `LineStyle::AskUser` 亮黄粗体（替代原来的 `LineStyle::Assistant`）
   - 操作提示行 `[↑↓] 选择  [Enter] 确认  [Esc] 取消`（多选时为 `[↑↓] 移动  [Space] 选中/取消`）
   - 默认选项用 `❯` 箭头 + AskUser 样式
   - 选项底部增加空行分隔
   - 无选项自由输入模式补齐操作提示
3. 优化 `update_ask_user_options()` 中光标/选中项的高亮样式为 `LineStyle::AskUser`
4. Spinner 暂停：`UiEvent::ToolCall` 处理中已对 `AskUserQuestion` 跳过 `start_spinner()`（已有逻辑，无需改动）

## 涉及文件

- `aemeath-cli/src/tui/app/update.rs`（阶段一；阶段二 spinner 跳过已有逻辑）
- `aemeath-cli/src/tui/output_area/types.rs`（阶段二）
- `aemeath-cli/src/tui/output_area/content.rs`（阶段二）

## 验证

- `cargo build` 编译通过
- `cargo test -p aemeath-cli` 110 passed, 0 failed
- 用户已确认完成
