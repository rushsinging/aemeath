//! Edit 工具结果的 diff 渲染：解析 `---DIFF---` 标记内的 old/new 文本，
//! 复用 `primitives::diff::diff`（行号 + 加减语义色 + 语法高亮 + 缩进）渲染为
//! `RenderedLine`，下游统一经 `apply_selection_overlay` 可选中并保留前景色（bug #61）。

use crate::tui::render::output::primitives::diff::diff;
use crate::tui::render::output::rendered::RenderedLine;
use crate::tui::render::syntax::extension_from_path;

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

/// 推断 Edit diff 的语法高亮扩展名。
///
/// 运行时 `view.title` 是裸工具名 `"Edit"`（无路径括号，见
/// `view_assembler/output.rs` 的 `title: call.name.clone()`），故 **不可**从 title 取。
/// 优先级：
/// 1. `summary`（工具入参 JSON，含 `file_path`，见 `adapter` 将 `input.to_string()`
///    存入 summary）。
/// 2. 退而从 Edit 结果 header 的 `in {path}` 解析（`agent/tools/src/file_edit.rs`
///    输出 `replaced N occurrence(s)[...] in {file_path}`）。
pub fn file_ext_for_edit(summary: Option<&str>, result: &str) -> Option<String> {
    file_ext_from_args(summary).or_else(|| file_ext_from_result_header(result))
}

/// 从工具入参 JSON 中取 `file_path` 的扩展名。
fn file_ext_from_args(summary: Option<&str>) -> Option<String> {
    let summary = summary?;
    let value: serde_json::Value = serde_json::from_str(summary).ok()?;
    let path = value.get("file_path")?.as_str()?;
    extension_from_path(path).map(str::to_string)
}

/// 从 Edit 结果 header 的 `in {path}` 子串解析扩展名。
fn file_ext_from_result_header(result: &str) -> Option<String> {
    // 仅取首行 header（DIFF 正文不含 "in " 路径语义，避免误判）。
    let header = result.lines().next()?;
    let path = header.rsplit_once(" in ")?.1.trim();
    extension_from_path(path).map(str::to_string)
}

/// 若 result 是 Edit diff，则渲染为带行号/语义色/语法高亮的 diff 行。
///
/// `summary`（工具入参 JSON）用于推断语法高亮语言；退而用 result header 的 `in {path}`。
/// `width` 传入 diff 原语。
pub fn render_edit_diff(
    summary: Option<&str>,
    result: &str,
    width: u16,
) -> Option<Vec<RenderedLine>> {
    let parsed = parse_edit_diff(result)?;
    let ext = file_ext_for_edit(summary, result);
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
    fn test_file_ext_for_edit_from_args_json() {
        // 正常路径：summary 是入参 JSON，含 file_path → 取扩展名。
        let summary = r#"{"file_path":"src/lib.rs","old_string":"a","new_string":"b"}"#;
        assert_eq!(
            file_ext_for_edit(Some(summary), "replaced 1 occurrence(s) in src/lib.rs").as_deref(),
            Some("rs")
        );
    }

    #[test]
    fn test_file_ext_for_edit_falls_back_to_result_header() {
        // summary 缺失/无 file_path → 从结果 header 的 "in {path}" 解析。
        let result = "replaced 2 occurrence(s) in /a/b/main.py\n---DIFF---\nx\n---DIFF---\ny";
        assert_eq!(file_ext_for_edit(None, result).as_deref(), Some("py"));
        assert_eq!(file_ext_for_edit(Some("{}"), result).as_deref(), Some("py"));
    }

    #[test]
    fn test_file_ext_for_edit_none_when_no_extension_or_no_source() {
        // 边界/错误：无扩展名、无 in 路径、非 JSON summary 均返回 None。
        assert!(file_ext_for_edit(Some("not json"), "no path here").is_none());
        assert!(file_ext_for_edit(Some(r#"{"file_path":"Makefile"}"#), "done").is_none());
        assert!(file_ext_for_edit(None, "replaced 1 occurrence(s) in Dockerfile").is_none());
    }

    #[test]
    fn test_render_edit_diff_emits_line_numbers_signs_indent_and_color() {
        let result = edit_result("let a = 1;", "let a = 2;");
        let summary = r#"{"file_path":"src/lib.rs"}"#;
        let lines = render_edit_diff(Some(summary), &result, 80).unwrap();

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
        assert!(render_edit_diff(Some(r#"{"file_path":"a.rs"}"#), "120 lines", 80).is_none());
    }

    #[test]
    fn test_render_edit_diff_does_not_contain_raw_marker() {
        let result = edit_result("a", "b");
        let lines = render_edit_diff(Some(r#"{"file_path":"x.rs"}"#), &result, 80).unwrap();

        assert!(
            lines.iter().all(|line| !line.plain.contains("---DIFF---")),
            "渲染后不应残留原始标记"
        );
    }

    #[test]
    fn test_render_edit_diff_real_bare_title_summary_drives_syntax_highlight() {
        // M1 回归：运行时 title 是裸 "Edit"（无括号路径），ext 必须从 summary 的
        // file_path 推断。注入真实 summary，断言 Rust 语法高亮被激活
        //（新增行因高亮产生 >2 个 span，而非单色 1 个内容 span）。
        // header 无可解析扩展名（Dockerfile），确保基线不会经 header 回退拿到 ext。
        let result =
            "edited Dockerfile\n---DIFF---\nfn old() {}\n---DIFF---\nfn new() {}".to_string();
        let summary = r#"{"file_path":"src/lib.rs","old_string":"fn old() {}"}"#;

        let with_ext = render_edit_diff(Some(summary), &result, 80).unwrap();
        let without_ext = render_edit_diff(Some("{}"), &result, 80).unwrap();

        // 新增行（含 "new"）。
        let added_with = with_ext
            .iter()
            .find(|l| l.plain.contains("new"))
            .expect("新增行存在");
        let added_without = without_ext
            .iter()
            .find(|l| l.plain.contains("new"))
            .expect("新增行存在");

        // 有 ext → 语法高亮产生更多 span；无 ext → 单色少 span。
        assert!(
            added_with.spans.len() > added_without.spans.len(),
            "summary 含 file_path 时应激活语法高亮（更多 span）: with={} without={}",
            added_with.spans.len(),
            added_without.spans.len()
        );
    }
}
