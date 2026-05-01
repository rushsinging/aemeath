# #7 Input Queue 优化

**归档日期**：2026-05-01

**确认结果**：用户确认完成

**实现**：
- 将单条 `queued_input` 改为 `VecDeque` 多消息队列。
- 支持处理期间连续提交多条输入并按原顺序排队。
- TUI 临时区域展示 queued messages，并在后续处理时刷新为正式用户消息。

**涉及文件**：
- `aemeath-cli/src/tui/app/mod.rs`
- `aemeath-cli/src/tui/app/update.rs`
- `aemeath-cli/src/tui/output_area/mod.rs`
