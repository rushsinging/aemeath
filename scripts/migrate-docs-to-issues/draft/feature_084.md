<!-- Migrated from: docs/feature/active.md#84 -->
### #84 Stop hook 命令显示短路径

**状态**：✅ 已完成

**背景 / 目标**：Stop hook 在 TUI 中显示执行命令时，当前会展示类似 `{AEMEATH_PROJECT_DIR}/build_cli.sh` 的完整模板路径，内容过长且项目路径变量对用户定位脚本帮助有限。目标是在用户可见的 hook stop 提示中只显示最后一级命令/脚本名，例如 `build_cli.sh`。

**设计方向**：
1. 仅调整 TUI/用户可见展示文本，不改变 hook 实际执行命令、环境变量注入或日志中的原始命令。
2. 对 hook 命令展示做路径 basename 化：包含 `/` 的命令只取最后一段；不含 `/` 的命令保持原样。
3. 需要覆盖 `{AEMEATH_PROJECT_DIR}/build_cli.sh`、绝对路径、相对路径、普通命令名等场景；必要时保留 tooltip/detail 或日志里的完整命令用于排查。

**验收标准**：
1. Stop hook 运行/阻止提示中显示 `build_cli.sh`，不再直接显示 `{AEMEATH_PROJECT_DIR}/build_cli.sh` 这类长路径。
2. Hook 执行行为不变，`AEMEATH_PROJECT_DIR` / `CLAUDE_PROJECT_DIR` 注入不受影响。
3. 完整命令仍可在日志或内部结果中用于调试，不因展示缩短而丢失执行信息。

**验证**：
- `CARGO_TARGET_DIR=target cargo test -p cli hook_notice`
- `CARGO_TARGET_DIR=target cargo check -p cli`
- `CARGO_TARGET_DIR=target cargo clippy -p cli --all-targets -- -D warnings`
- `cargo fmt --check`
- `git diff --check`

**涉及路径**：
- `apps/cli/src/tui/**`
- `agent/features/hook/src/business/hook/**`
