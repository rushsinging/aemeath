# Feature #75 EnterWorktree/ExitWorktree result 不截断

**状态**：已完成 / 已归档

**修复 commits**：
- `d9a70208` fix(tui): worktree 工具结果完整展示 (refs #75)
- `d5a1b8e5` merge: feature/75-worktree-result-full-display — 完整展示 worktree 工具结果 (refs #75)

## 症状

EnterWorktree / ExitWorktree 的工具结果是固定的工作区上下文提示，通常只有少量行；默认 `TOOL_RESULT_MAX_LINES = 5` 会导致 TUI 输出区显示 `... (n lines omitted)`，隐藏后续关于 path_base / working_root 使用约束的关键提示。

## 根因

TUI tool result 渲染层对所有工具使用统一默认行数上限，没有针对固定短上下文类结果进行工具级覆盖。

## 修复方案

1. 保持全局默认工具结果预览行数不变，避免影响 Bash / Read / Grep 等可能产生大输出的工具。
2. 为 `EnterWorktreeDisplay` 与 `ExitWorktreeDisplay` 单独覆盖 `result_max_lines()`，允许完整展示固定上下文结果。
3. 新增回归测试覆盖 EnterWorktree / ExitWorktree 结果不出现 `lines omitted`，且仍展示最后一条工作区路径使用提示。

## 验证

- `cargo test -p cli test_render_tool_result_worktree_tools_do_not_truncate_fixed_context_result`
- `cargo test -p cli tool_result`
- `cargo fmt --check`
- `cargo check -p cli`

## 涉及路径

- `apps/cli/src/tui/render/output/tool_display/tool_impls.rs`
- `apps/cli/src/tui/render/output/blocks/tool_result.rs`
