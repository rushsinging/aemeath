<!-- Migrated from: docs/feature/archived/072-edit-diff-real-line-numbers.md -->
# Feature #72：Edit diff 显示真实文件行号

| 字段 | 值 |
|------|-----|
| 优先级 | 中 |
| 登记日期 | 2026-05 |
| 归档日期 | 2026-06-04 |
| 状态 | 已确认完成 |

## 背景

Edit 工具结果的 diff 渲染中，行号是 `old_string`/`new_string` 片段内部从 1 开始计数的相对行号。例如修改文件第 100-105 行，diff 显示 1-5 而非 100-105，用户无法直接定位到文件真实位置。

## 实现

1. `FileEditTool` 在写入前通过 `content.find(&matched_old)` 计算匹配片段在原文件中的 1-based 起始行号
2. Edit 工具结果 diff 标记从 `---DIFF---` 扩展为 `---DIFF:LINE:{start_line}---`
3. `parse_edit_diff` 支持新标记并向后兼容旧 `---DIFF---`，旧格式默认 `start_line = 1`
4. `diff_from` / `build_diff_lines_from` 接收 old/new 起始行号，行号计数器从 `start - 1` 开始，列宽按真实最大行号计算
5. 既有 `diff` / `build_diff_lines` 保留旧签名并委托到起始行号 1，避免影响非 Edit 调用

## 涉及路径

- `agent/features/tools/src/business/file_edit.rs`
- `apps/cli/src/tui/render/output/blocks/edit_diff.rs`
- `apps/cli/src/tui/render/output/diff.rs`
- `apps/cli/src/tui/render/output/primitives/diff.rs`

## 验证

- `cargo check`
- `cargo test -p cli tui::render::output::diff::tests`
- `cargo test -p cli tui::render::output::blocks::edit_diff::tests`
- `cargo test -p cli tui::render::output::primitives::diff::tests`
- `cargo test -p tools test_start_line_of_match`
- `cargo test -p tools test_file_edit_success_diff_marker_includes_real_line_number`
- 用户确认完成。

## 关联提交

- `12029c3 feat(tui): Edit diff 显示真实文件行号 (refs #72)`
- `1fc4a3c fix(tui): 标注 Edit diff ASCII marker 切片 (refs #72)`
- `f28df49 merge: feature/72-real-edit-diff-line-numbers (refs #72)`
