# Feature #77: diff removed 行纯红显示

## 背景

TUI diff 渲染当前会对 removed/delete 行正文调用 syntect 语法高亮，导致删除行内部出现多种语法颜色。Feature #77 要求 removed/delete 行不再语法高亮，只显示纯 `DIFF_REMOVE_FG` 红色。

## 目标

1. unified diff 中 `DiffLineKind::Removed` 行正文 MUST 使用纯 `DIFF_REMOVE_FG`。
2. 普通 diff 中 `ChangeTag::Delete` 行正文 MUST 使用纯 `DIFF_REMOVE_FG`。
3. removed/delete 行 MUST NOT 调用语法高亮 helper。
4. added/context 行 MUST 保持现有语法高亮行为。
5. diff 行号、gutter、prefix/marker、plain text 内容 MUST 保持现状。

## 设计

采用用户指定的备选方案 2：新增 deleted/removed 专用 helper。

### 普通 diff

在 `apps/cli/src/tui/render/output/diff.rs` 中：

- 新增 `push_deleted_text(spans, text)` helper。
- `build_delete_line()` 保留行号、分隔符和 `- ` marker 逻辑。
- `build_delete_line()` 改为调用 `push_deleted_text()`。
- `push_deleted_text()` 只追加 `SpanPart::plain(text.to_string(), DIFF_REMOVE_FG)`，不接受 `syntax_ref`，不调用 `push_highlighted_text()`。

### unified diff

在 `apps/cli/src/tui/render/output/primitives/unified_diff.rs` 中：

- 新增 `push_removed_body(parts, body)` helper。
- `DiffLineKind::Removed` arm 保留 `-` prefix。
- `DiffLineKind::Removed` arm 改为调用 `push_removed_body()`。
- `push_removed_body()` 只追加 `SpanPart::plain(body.to_string(), theme::DIFF_REMOVE_FG)`，不接受 `syntax_ref`，不调用 `push_highlighted_body()`。

## 测试

1. unified diff：removed 行在传入 `Some("rs")` 时仍只产生缩进、`-`、正文等少量纯红 span，不产生 syntect 多色 span。
2. unified diff：added/context 行继续产生语法高亮 span。
3. 普通 diff：delete 行在传入 `Some("rs")` 时正文 span 全部为 `DIFF_REMOVE_FG`。
4. 普通 diff：insert/context 行继续使用现有高亮逻辑。

## 非目标

1. 不改 added/context 行高亮。
2. 不改 diff 行号、gutter、缩进、wrap。
3. 不重构 diff 渲染架构。
4. 不改变 `push_highlighted_text` / `push_highlighted_body` 的现有语义。
