# Feature #76：TUI spinner 时长显示改进

| 字段 | 值 |
|------|-----|
| 优先级 | 低 |
| 登记日期 | 2026-06 |
| 归档日期 | 2026-06-04 |
| 状态 | 已确认完成 |

## 背景

旧版 spinner 渲染：
- verb 后缀使用 Unicode 省略号 `…`
- spinner 行括号内只显示 `phase_text`（如 `Thinking...`），没有时长
- 用户无法直观看到当前阶段已执行多久

## 实现

1. **verb 后缀 `…` → `...`**：与项目通用文案保持一致
2. **括号内追加 `⏱ Xs` 计时**：spinner 行格式变为 `<verb>...  <total>s  (<emoji> <phase>...  ⏱ <phase_elapsed>s)`
3. **phase 计时随 phase 变化重置**：
   - `SpinnerState` 新增 `phase_start: Instant`
   - `live_status_widget` 在 phase 文本变化时重置 phase_start，verb 后的总时长继续用 `start` 不变
4. **phase_text 加 emoji 前缀**：
   - 🧠 Thinking / Reflecting / Thinking with queued input
   - ✍️ Generating
   - 🤖 Agent working
   - 🔧 Calling `<tool>` / Calling tools
   - 🪝 Hook `<event>`（running/blocked/done/failed）

## 涉及路径

- `apps/cli/src/tui/render/output_area/spinner.rs`
- `apps/cli/src/tui/render/output_area/types.rs`（SpinnerState 新增 phase_start）
- `apps/cli/src/tui/adapter/live_status_widget.rs`（phase 变化重置）
- `apps/cli/src/tui/view_assembler/live_status.rs`（emoji 前缀）
- `apps/cli/src/tui/render/output/status_line.rs`（测试构造）

## 验证

- `cargo check -p cli`
- `cargo test -p cli`（661 passed / 0 failed）
- 用户确认完成。

## 关联提交

- `14172d0 feat(tui): spinner 时长显示改进 (refs #71)`
- `1309936 merge: feature/hook-spinner-duration (refs #71)`
- `b909bc9 feat(tui): spinner phase 计时随 phase 变化重置 (refs #71)`
- `2b30c1c merge: feature/spinner-phase-elapsed-reset (refs #71)`
- `6f6d28a feat(tui): spinner phase 文本添加 emoji 前缀 (refs #71)`
- `de1a0bc merge: feature/spinner-phase-emoji (refs #71)`

> 说明：commit refs 用了 `#71`，但 active.md 实际登记编号为 `#76`（同期 `#71` 用于 Stop hook 日志输出 feature）。
