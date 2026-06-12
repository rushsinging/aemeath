<!-- Migrated from: docs/feature/active.md#77 -->
### #77 diff removed 行不语法高亮，只显示纯红色

**状态**：待确认

**背景**：diff removed/delete 行当前会把正文交给 syntect 语法高亮，导致删除行中出现关键字、标点等语法色。Feature #77 要求删除语义行保持单一删除色，避免红色删除语义被语法色冲淡。

**设计**：
1. 采用 deleted/removed 专用 helper，而不是给通用高亮 helper 增加开关。
2. unified diff 的 `DiffLineKind::Removed` 只保留 `-` prefix 与纯 `DIFF_REMOVE_FG` 正文 span。
3. 普通 diff 的 `build_delete_line` 不再接收 `syntax_ref`，避免误用语法高亮。
4. added/context 行继续沿用现有 syntect 高亮逻辑。

**验证**：
- `cargo test -p cli unified_diff -- --nocapture`
- `cargo test -p cli diff -- --nocapture`

**涉及路径**：
- `apps/cli/src/tui/render/output/primitives/unified_diff.rs`
- `apps/cli/src/tui/render/output/diff.rs`
