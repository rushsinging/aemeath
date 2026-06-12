<!-- Migrated from: docs/feature/active.md#81 -->
### #81 TUI assistant 文本与 spinner phase 视觉调整

**状态**：待确认

**症状 / 目标**：spinner phase 文案前的 emoji 造成状态行视觉噪音；assistant 正文当前没有行首标识，和周边 block 的层级提示不一致。目标是 spinner phase 只显示纯文本，同时 assistant 文本首行前显示白色圆点 gutter。

**根因 / 设计点**：
1. Spinner phase 文案集中在 `LiveStatusAssembler::phase_text()`，该函数直接拼入 emoji 前缀。
2. 输出区 gutter 的 marker 由 `marker_glyph()` 按 `OutputBlockKind` 映射；`AssistantMessage` 当前落入默认空 marker。

**实现**：
1. 将 spinner phase 文案改为纯文本：`Thinking...`、`Generating...`、`Calling <tool>...`、`Hook <event>...`。
2. 为 `OutputBlockKind::AssistantMessage` 映射静态 `●` marker，颜色使用 `theme::ASSISTANT`。
3. 保持 gutter 只进入 spans、不进入 plain；续行继续使用等宽空白，不影响复制和选区坐标。

**验证**：
- `CARGO_TARGET_DIR=target cargo test -p cli live_status`
- `CARGO_TARGET_DIR=target cargo test -p cli gutter`
- `cargo fmt --check`
- `CARGO_TARGET_DIR=target cargo check -p cli`
- `CARGO_TARGET_DIR=target cargo clippy -p cli --all-targets -- -D warnings`

**涉及路径**：
- `apps/cli/src/tui/view_assembler/live_status.rs`
- `apps/cli/src/tui/adapter/live_status_widget.rs`
- `apps/cli/src/tui/render/output/gutter.rs`
