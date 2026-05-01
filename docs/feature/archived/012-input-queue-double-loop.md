# #12 Input Queue 双层循环优化

**归档日期**：2026-05-01

**确认结果**：用户确认完成

**目标**：让 LLM 不必等完整一轮 assistant 响应结束后才处理用户排队输入。尤其是在 tool call 完成后，如果用户已经在 input queue 中补充了新要求，立即把这些输入追加到下一次 LLM 调用中，让模型基于最新反馈继续执行。

**实现**：
- 后台 agent loop 在 tool batch 完成并同步 tool results 后，通过 `DrainQueuedInput` 向 TUI 主循环请求清空当前 input queue。
- TUI 主循环按原入队顺序返回 queued messages，并将临时 queued messages 刷新为正式用户消息显示。
- 后台 loop 将这些 queued messages 追加为新的 `Message::user`，在下一次 LLM API 调用前同步 session。
- 若用户没有排队输入，则保持原流程不变。

**涉及文件**：
- `aemeath-cli/src/tui/app/mod.rs`
- `aemeath-cli/src/tui/app/processing.rs`
- `aemeath-cli/src/tui/app/stream.rs`
- `aemeath-cli/src/tui/app/update.rs`
- `aemeath-cli/src/tui/app/event_handler.rs`
- `aemeath-cli/src/tui/app/input_handler.rs`
