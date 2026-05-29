//! Edit 工具结果的 diff 渲染：解析 `---DIFF---` 标记内的 old/new 文本，
//! 复用 `primitives::diff::diff`（行号 + 加减语义色 + 语法高亮 + 缩进）渲染为
//! `RenderedLine`，下游统一经 `apply_selection_overlay` 可选中并保留前景色（bug #61）。

use crate::tui::render::output::primitives::diff::diff;
use crate::tui::render::output::rendered::RenderedLine;

/// Edit 工具结果中包裹 old/new 文本的标记。
const DIFF_MARKER: &str = "---DIFF---";

/// 解析后的 Edit diff 数据：变更前/后文本。
pub struct EditDiff {
    pub old: String,
    pub new: String,
}

/// 从 Edit 工具结果文本中解析出 old/new 两份文本。
///
/// 期望格式（见 `agent/tools/src/file_edit.rs`）：
/// ```text
/// replaced N occurrence(s) in {path}
/// ---DIFF---
/// {old}
/// ---DIFF---
/// {new}
/// ```
/// 未命中标记结构时返回 None（调用方退回纯文本渲染）。
pub fn parse_edit_diff(result: &str) -> Option<EditDiff> {
    let mut parts = result.splitn(3, DIFF_MARKER);
    let _header = parts.next()?;
    let old = parts.next()?;
    let new = parts.next()?;
    Some(EditDiff {
        old: strip_edge_newlines(old).to_string(),
        new: strip_edge_newlines(new).to_string(),
    })
}

/// 去除标记前后插入的单个换行符，保留内部内容原样。
fn strip_edge_newlines(text: &str) -> &str {
    let text = text.strip_prefix('\n').unwrap_or(text);
    text.strip_suffix('\n').unwrap_or(text)
}

/// 从工具标题（如 `Edit(src/lib.rs)` 或 `● Edit(/a/b.py)`）中推断文件扩展名。
pub fn file_ext_from_title(title: &str) -> Option<String> {
    let path = title.split_once('(')?.1.split_once(')')?.0;
    path.rsplit('.')
        .next()
        .filter(|ext| !ext.is_empty() && *ext != path)
        .map(|ext| ext.to_string())
}

/// 若 result 是 Edit diff，则渲染为带行号/语义色/语法高亮的 diff 行。
///
/// `title` 用于推断语法高亮语言，`width` 传入 diff 原语。
pub fn render_edit_diff(title: &str, result: &str, width: u16) -> Option<Vec<RenderedLine>> {
    let parsed = parse_edit_diff(result)?;
    let ext = file_ext_from_title(title);
    Some(diff(&parsed.old, &parsed.new, ext.as_deref(), width))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edit_result(old: &str, new: &str) -> String {
        format!("replaced 1 occurrence(s) in src/lib.rs\n---DIFF---\n{old}\n---DIFF---\n{new}")
    }

    #[test]
    fn test_parse_edit_diff_extracts_old_and_new() {
        let parsed = parse_edit_diff(&edit_result("let a = 1;", "let a = 2;")).unwrap();

        assert_eq!(parsed.old, "let a = 1;");
        assert_eq!(parsed.new, "let a = 2;");
    }

    #[test]
    fn test_parse_edit_diff_multiline_preserves_inner_content() {
        let old = "fn f() {\n    1\n}";
        let new = "fn f() {\n    2\n}";
        let parsed = parse_edit_diff(&edit_result(old, new)).unwrap();

        assert_eq!(parsed.old, old);
        assert_eq!(parsed.new, new);
    }

    #[test]
    fn test_parse_edit_diff_returns_none_without_marker() {
        assert!(parse_edit_diff("wrote 10 bytes to a.txt").is_none());
        assert!(parse_edit_diff("done: 3 matches").is_none());
    }

    #[test]
    fn test_file_ext_from_title_rust() {
        assert_eq!(file_ext_from_title("Edit(src/lib.rs)").as_deref(), Some("rs"));
        assert_eq!(
            file_ext_from_title("● Edit(/a/b/main.py)").as_deref(),
            Some("py")
        );
    }

    #[test]
    fn test_file_ext_from_title_no_extension_or_no_parens() {
        assert!(file_ext_from_title("Edit(Makefile)").is_none());
        assert!(file_ext_from_title("Edit()").is_none());
        assert!(file_ext_from_title("Grep /foo/").is_none());
    }

    #[test]
    fn test_render_edit_diff_emits_line_numbers_signs_indent_and_color() {
        let result = edit_result("let a = 1;", "let a = 2;");
        let lines = render_edit_diff("Edit(src/lib.rs)", &result, 80).unwrap();

        let plains: Vec<&str> = lines.iter().map(|line| line.plain.as_str()).collect();

        // 删除行带 "- " 与原文本，新增行带 "+ " 与新文本（加减语义）。
        assert!(
            plains.iter().any(|p| p.contains("- ") && p.contains("1;")),
            "应含删除行，got: {plains:?}"
        );
        assert!(
            plains.iter().any(|p| p.contains("+ ") && p.contains("2;")),
            "应含新增行，got: {plains:?}"
        );
        // 行号（两空格缩进开头）。
        assert!(
            lines.iter().all(|line| line.plain.starts_with("  ")),
            "每行保留两空格缩进"
        );
        // 至少一行带前景色 span（语义色 / 语法高亮）。
        assert!(
            lines
                .iter()
                .any(|line| line.spans.iter().any(|span| span.style.fg.is_some())),
            "应有带前景色的 span"
        );
    }

    #[test]
    fn test_render_edit_diff_none_for_non_diff_result() {
        assert!(render_edit_diff("Read(a.rs)", "120 lines", 80).is_none());
    }

    #[test]
    fn test_render_edit_diff_does_not_contain_raw_marker() {
        let result = edit_result("a", "b");
        let lines = render_edit_diff("Edit(x.rs)", &result, 80).unwrap();

        assert!(
            lines.iter().all(|line| !line.plain.contains("---DIFF---")),
            "渲染后不应残留原始标记"
        );
    }
}
