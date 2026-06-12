<!-- Migrated from: docs/feature/archived/016-spinner-merged-status-hook-info.md -->
# #16 Spinner 行合并状态显示 + Hook 调用信息

**归档日期**：2026-05-02

**确认结果**：用户确认完成

**目标**：把 status line 上承担的 Thinking / Calling xxx / Generating 等运行态信息搬到 output area 的 spinner 行展示，让用户只需关注 spinner 行即可了解 agent 当前阶段；同时把 hook 触发信息也并入 spinner 行展示。

**实现**：
- spinner 行合并展示当前运行阶段，例如 Thinking、Generating、Calling 工具、Compacting、Waiting for user。
- Hook 运行态接入 spinner 行，可展示当前 hook event 与 hook 名称。
- status bar 保留 provider / model / cwd / token / cost 等环境与累计信息，不再承担瞬时运行态展示。
- /clear 等重置路径同步清理 spinner 与运行态字段，避免旧状态残留。

**涉及文件**：
- `aemeath-cli/src/tui/output_area/spinner.rs`
- `aemeath-cli/src/tui/app/update.rs`
- `aemeath-cli/src/tui/app/mod.rs`
- `aemeath-cli/src/tui/status_bar.rs`
- `aemeath-core/src/hook.rs`
- `aemeath-core/src/config/`
