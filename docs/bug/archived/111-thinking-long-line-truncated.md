# #111 LLM 输出长行被截断，TUI 只显示到屏幕宽度即断行消失

**状态**：已修复

**症状**：LLM 输出长行在 TUI 中曾只显示到屏幕宽度，超出部分不可见；首轮修复后普通正文长行可换行，但 reasoning/thinking 文本仍会被截断。例如中文问候触发的 reasoning 内容只显示到 `The user is greeting me in Chinese... a simple g`，后续内容没有自动换行显示。

**根因**：首轮修复只统一了 output document 渲染宽度并预留 scrollbar 右侧安全留白。普通 assistant message 走 markdown/fenced markdown 渲染，会按 `ctx.width` 预换行；但 `thinking.rs` 直接按原始 `text.lines()` 生成 `RenderedLine`，完全未使用 `ctx.width`，长 reasoning 行交给 ratatui `Paragraph` 后被当前可见宽度截断。

**修复**：thinking block 复用 inline markdown 的显示宽度换行逻辑，保留 `theme::THINKING` 样式和 gutter marker 语义；补充窄宽度下长 reasoning 文本会拆成多行且每行不超过渲染宽度的回归测试。

**验证**：
- `cargo fmt --check`
- `git diff --check`
- `cargo test -p cli thinking`
- `cargo test -p cli assistant`

**涉及路径**：
- `apps/cli/src/tui/app/update.rs`
- `apps/cli/src/tui/app.rs`
