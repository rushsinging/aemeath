//! unified diff 原语：识别 LLM markdown ` ```diff ` 代码块内的统一 diff 文本，
//! 复用 `output/diff.rs` 的 `INDENT` 缩进 + `DIFF_ADD_FG`/`DIFF_REMOVE_FG` 语义色风格
//! 渲染为 `RenderedLine`（bug #61：修复 diff 行贴最左、选中高亮丢失）。
//!
//! 与 `primitives::diff::diff`（基于 `similar` 重算行号）不同：unified diff 文本自带
//! `@@ ... @@` 行号信息，按原文呈现即可，不重算行号；added 行（去前导 `+`）可选语法高亮。

use crate::tui::render::output::primitives::spanparts_to_spans;
use crate::tui::render::output::rendered::RenderedLine;
use crate::tui::render::output_area::types::{SpanPart, INDENT};
use crate::tui::render::syntax::{self, language_by_extension};
use crate::tui::render::theme;
use ratatui::style::Color;

/// unified diff 行类型。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DiffLineKind {
    /// `@@ -a,b +c,d @@` hunk 头。
    Hunk,
    /// `+` 起始的新增行（但非 `+++` 文件头）。
    Added,
    /// `-` 起始的删除行（但非 `---` 文件头）。
    Removed,
    /// 文件头 `+++`/`---`/`diff `/`index ` 等元信息。
    Meta,
    /// 上下文行及其它。
    Context,
}

/// 识别单行 unified diff 类型。
fn classify(line: &str) -> DiffLineKind {
    if line.starts_with("@@") {
        DiffLineKind::Hunk
    } else if line.starts_with("+++")
        || line.starts_with("---")
        || line.starts_with("diff ")
        || line.starts_with("index ")
    {
        DiffLineKind::Meta
    } else if line.starts_with('+') {
        DiffLineKind::Added
    } else if line.starts_with('-') {
        DiffLineKind::Removed
    } else {
        DiffLineKind::Context
    }
}

/// 渲染一段 unified diff 文本为带缩进 + 加减语义色（+ 可选语法高亮）的渲染行。
///
/// `ext` 用于对新增行做语法高亮（去掉前导 `+` 后高亮再补回 `+ `），None 不高亮。
/// `_width` 预留参数（与 `primitives::diff::diff` 签名对齐），当前不换行。
pub fn render_unified_diff(text: &str, ext: Option<&str>, _width: u16) -> Vec<RenderedLine> {
    let syntax_ref = ext.and_then(language_by_extension);
    text.lines()
        .map(|line| render_line(line, syntax_ref.as_ref()))
        .collect()
}

/// 单行渲染：保持 `INDENT` 缩进（修 #61 贴最左），按类型着色。
fn render_line(line: &str, syntax_ref: Option<&syntect::parsing::SyntaxReference>) -> RenderedLine {
    let kind = classify(line);
    let mut parts: Vec<SpanPart> = vec![SpanPart::plain(INDENT.to_string(), theme::TEXT_DIM)];
    match kind {
        DiffLineKind::Hunk => {
            parts.push(SpanPart::plain(line.to_string(), theme::TEXT_DIM));
        }
        DiffLineKind::Meta => {
            parts.push(SpanPart::plain(line.to_string(), theme::TEXT_MUTED));
        }
        DiffLineKind::Removed => {
            parts.push(SpanPart::plain(line.to_string(), theme::DIFF_REMOVE_FG));
        }
        DiffLineKind::Added => {
            // 去掉前导 '+'（单 ASCII 字节）做语法高亮，再补回 '+' 前缀语义符号。
            let body = line.strip_prefix('+').unwrap_or(line);
            parts.push(SpanPart::plain("+".to_string(), theme::DIFF_ADD_FG));
            push_highlighted_body(&mut parts, body, theme::DIFF_ADD_FG, syntax_ref);
        }
        DiffLineKind::Context => {
            parts.push(SpanPart::plain(line.to_string(), theme::TEXT));
        }
    }
    RenderedLine::new(spanparts_to_spans(&parts))
}

/// 将 body 高亮后追加；无语法引用或高亮失败时回退为 `fallback` 单色。
fn push_highlighted_body(
    parts: &mut Vec<SpanPart>,
    body: &str,
    fallback: Color,
    syntax_ref: Option<&syntect::parsing::SyntaxReference>,
) {
    if let Some(highlighted) = syntax::highlight_line(body, syntax_ref) {
        parts.extend(highlighted);
    } else {
        parts.push(SpanPart::plain(body.to_string(), fallback));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "@@ -1,3 +1,3 @@\n context\n-let a = 1;\n+let a = 2;";

    #[test]
    fn test_render_unified_diff_indents_and_colors_by_kind() {
        let lines = render_unified_diff(SAMPLE, None, 80);

        // 每行保留两空格缩进（修 #61 贴最左）。
        assert!(
            lines.iter().all(|line| line.plain.starts_with("  ")),
            "每行应保留 INDENT 缩进, got: {:?}",
            lines.iter().map(|l| l.plain.as_str()).collect::<Vec<_>>()
        );
        // hunk 头行存在且为 dim 色。
        let hunk = lines.iter().find(|l| l.plain.contains("@@")).unwrap();
        assert!(hunk
            .spans
            .iter()
            .any(|s| s.style.fg == Some(theme::TEXT_DIM)));
        // 删除行带 remove 语义色。
        let removed = lines.iter().find(|l| l.plain.contains("1;")).unwrap();
        assert!(
            removed
                .spans
                .iter()
                .any(|s| s.style.fg == Some(theme::DIFF_REMOVE_FG)),
            "删除行应带 DIFF_REMOVE_FG"
        );
        // 新增行带 add 语义色（前缀 + 符号）。
        let added = lines.iter().find(|l| l.plain.contains("2;")).unwrap();
        assert!(
            added
                .spans
                .iter()
                .any(|s| s.style.fg == Some(theme::DIFF_ADD_FG)),
            "新增行应带 DIFF_ADD_FG"
        );
    }

    #[test]
    fn test_render_unified_diff_added_line_syntax_highlight() {
        let lines = render_unified_diff("+fn main() {}", Some("rs"), 80);
        let added = &lines[0];

        // 仍带 INDENT + 前缀 +，且因语法高亮产生多个 span。
        assert!(added.plain.starts_with("  +"));
        assert!(added.plain.contains("fn main"));
        assert!(
            added.spans.len() > 2,
            "语法高亮应产生多个 span, got {}",
            added.spans.len()
        );
    }

    #[test]
    fn test_render_unified_diff_meta_and_file_headers_not_treated_as_add_remove() {
        let text = "diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -0,0 +1 @@\n+new";
        let lines = render_unified_diff(text, None, 80);

        // `---`/`+++` 文件头按 Meta（TEXT_MUTED），不当成删除/新增语义色。
        let minus_header = lines.iter().find(|l| l.plain.contains("--- a/x")).unwrap();
        assert!(
            minus_header
                .spans
                .iter()
                .all(|s| s.style.fg != Some(theme::DIFF_REMOVE_FG)),
            "--- 文件头不应着删除色"
        );
        let plus_header = lines.iter().find(|l| l.plain.contains("+++ b/x")).unwrap();
        assert!(
            plus_header
                .spans
                .iter()
                .all(|s| s.style.fg != Some(theme::DIFF_ADD_FG)),
            "+++ 文件头不应着新增色"
        );
        // 真正的新增行才着新增色。
        let added = lines.iter().find(|l| l.plain.ends_with("new")).unwrap();
        assert!(added
            .spans
            .iter()
            .any(|s| s.style.fg == Some(theme::DIFF_ADD_FG)));
    }

    #[test]
    fn test_render_unified_diff_empty_text() {
        let lines = render_unified_diff("", None, 80);
        assert!(lines.is_empty(), "空 diff 文本应产出 0 行");
    }

    #[test]
    fn test_render_unified_diff_pure_context_no_hunk_header() {
        // 无 @@ hunk 头、纯 context：每行 INDENT + TEXT 色，不误判加减。
        let lines = render_unified_diff("just text\nmore text", None, 80);

        assert_eq!(lines.len(), 2);
        assert!(lines.iter().all(|l| l.plain.starts_with("  ")));
        assert!(lines.iter().all(|l| l
            .spans
            .iter()
            .all(|s| s.style.fg != Some(theme::DIFF_ADD_FG)
                && s.style.fg != Some(theme::DIFF_REMOVE_FG))));
    }
}
