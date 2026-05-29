//! fenced markdown 原语：解析含 fenced code block 的多行文本为渲染行。
//!
//! 状态机（in_fence / fence_lang）是函数局部状态，随调用结束销毁，
//! 因此天然隔离跨调用的 code 样式泄漏（修 #65 的结构基础）。
//!
//! 行为：
//! - ``` ``` `` 围栏行：切换 fence 状态，按 TEXT_DIM 着色围栏标记本身。
//! - fence 内：``` ```diff `` 走 unified diff 渲染；否则按 fence_lang 语法高亮，
//!   无语言信息时按 CODE 单色。
//! - fence 外：表格走 table 原语，普通行走 inline markdown。
//!
//! `indent` 前缀加在每行最前（assistant 传 ""，工具结果传 INDENT），
//! 用于缩进对齐，且保证 `plain` 与可见 spans 一致。

use crate::tui::render::output::markdown::{is_table_row, is_table_separator};
use crate::tui::render::output::primitives::{
    markdown::markdown, rendered_line_from_spanparts, table::table, unified_diff::render_unified_diff,
};
use crate::tui::render::output::rendered::RenderedLine;
use crate::tui::render::{syntax, theme};
use ratatui::style::Style;
use ratatui::text::Span;

/// 解析含 fenced code block 的多行文本为渲染行。
///
/// - `text`：多行原始文本。
/// - `base_style`：fence 外普通文本的基色（assistant 用 ASSISTANT，工具结果用结果色）。
/// - `indent`：每行前缀（缩进），空串表示不缩进。
/// - `width`：可用宽度，传给 markdown / table 做换行。
pub fn render_fenced_markdown(
    text: &str,
    base_style: Style,
    indent: &str,
    width: u16,
) -> Vec<RenderedLine> {
    let src = text.lines().collect::<Vec<_>>();
    let mut lines: Vec<RenderedLine> = Vec::new();
    let mut idx = 0;
    let mut in_fence = false;
    let mut fence_lang: Option<String> = None;

    while idx < src.len() {
        let line = src[idx];
        let trimmed = line.trim_start();

        if trimmed.starts_with("```") {
            if in_fence {
                in_fence = false;
                fence_lang = None;
            } else {
                in_fence = true;
                fence_lang = Some(trimmed.trim_start_matches('`').trim().to_string());
            }
            lines.push(prefix_line(
                indent,
                base_style,
                vec![Span::styled(
                    line.to_string(),
                    Style::default().fg(theme::TEXT_DIM),
                )],
            ));
            idx += 1;
            continue;
        }

        if in_fence {
            // ` ```diff ` 代码块走 unified diff 渲染（行号信息来自 @@ 原文）。
            if fence_lang.as_deref() == Some("diff") {
                for rl in render_unified_diff(line, None, width) {
                    lines.push(prepend_indent(indent, base_style, rl));
                }
                idx += 1;
                continue;
            }
            let syntax_ref = fence_lang
                .as_deref()
                .and_then(syntax::language_by_extension);
            if let Some(parts) = syntax::highlight_line(line, syntax_ref.as_ref()) {
                lines.push(prepend_indent(
                    indent,
                    base_style,
                    rendered_line_from_spanparts(&parts),
                ));
            } else {
                lines.push(prefix_line(
                    indent,
                    base_style,
                    vec![Span::styled(
                        line.to_string(),
                        Style::default().fg(theme::CODE),
                    )],
                ));
            }
            idx += 1;
            continue;
        }

        if is_table_row(line) && idx + 1 < src.len() && is_table_separator(src[idx + 1]) {
            let mut end = idx;
            while end < src.len() && is_table_row(src[end]) {
                end += 1;
            }
            let block_src: Vec<&str> = src.iter().skip(idx).take(end - idx).copied().collect();
            for rl in table(&block_src, base_style, width) {
                lines.push(prepend_indent(indent, base_style, rl));
            }
            idx = end;
            continue;
        }

        for rl in markdown(line, base_style, width) {
            lines.push(prepend_indent(indent, base_style, rl));
        }
        idx += 1;
    }

    lines
}

/// 用 indent 前缀（着 base_style）包裹一组 spans 为一行。
fn prefix_line(indent: &str, base_style: Style, spans: Vec<Span<'static>>) -> RenderedLine {
    if indent.is_empty() {
        return RenderedLine::new(spans);
    }
    let mut out = vec![Span::styled(indent.to_string(), base_style)];
    out.extend(spans);
    RenderedLine::new(out)
}

/// 在已渲染行前补 indent 前缀，保持其 plain 与 spans 一致。
fn prepend_indent(indent: &str, base_style: Style, line: RenderedLine) -> RenderedLine {
    if indent.is_empty() {
        return line;
    }
    let mut spans = vec![Span::styled(indent.to_string(), base_style)];
    spans.extend(line.spans);
    RenderedLine::with_plain(spans, format!("{indent}{}", line.plain))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(text: &str) -> Vec<RenderedLine> {
        render_fenced_markdown(text, Style::default().fg(theme::TEXT), "", 80)
    }

    #[test]
    fn test_fenced_normal_line_uses_base_style_not_code() {
        // 正常路径：fence 外普通行不应被着成 CODE 色。
        let lines = render("hello world");

        assert_eq!(lines.len(), 1);
        assert_ne!(lines[0].spans[0].style.fg, Some(theme::CODE));
        assert!(lines[0].plain.contains("hello world"));
    }

    #[test]
    fn test_fenced_code_line_inside_fence_is_code_color() {
        // 正常路径：无语言围栏内的行按 CODE 单色。
        let lines = render("```\ncode line\n```");
        let code = lines
            .iter()
            .find(|l| l.plain.contains("code line"))
            .unwrap();

        assert_eq!(code.spans[0].style.fg, Some(theme::CODE));
    }

    #[test]
    fn test_fenced_does_not_leak_code_color_after_close() {
        // 核心回归（#65）：fence 关闭后普通行不得残留 CODE 色。
        let lines = render("```\ncode\n```\nafter");
        let after = lines.last().unwrap();

        assert_eq!(after.plain, "after");
        assert_ne!(after.spans[0].style.fg, Some(theme::CODE));
    }

    #[test]
    fn test_fenced_unclosed_fence_treats_rest_as_code() {
        // 边界：无闭合围栏——剩余行全部视为代码（状态机不 panic，行数正确）。
        let lines = render("```\nline1\nline2");

        assert_eq!(lines.len(), 3);
        let l2 = lines.iter().find(|l| l.plain.contains("line2")).unwrap();
        assert_eq!(l2.spans[0].style.fg, Some(theme::CODE));
    }

    #[test]
    fn test_fenced_empty_text_yields_no_lines() {
        // 边界：空文本产出空行集合。
        let lines = render("");

        assert!(lines.is_empty());
    }

    #[test]
    fn test_fenced_multiple_fences_alternate_correctly() {
        // 边界：多个围栏交替——两段普通行均不残留 CODE 色，中间代码行是 CODE 色。
        let lines = render("a\n```\ncode\n```\nb\n```\ncode2\n```\nc");

        for plain in ["a", "b", "c"] {
            let l = lines.iter().find(|l| l.plain == plain).unwrap();
            assert_ne!(
                l.spans[0].style.fg,
                Some(theme::CODE),
                "普通行 {plain} 不应为 CODE 色"
            );
        }
        let code = lines.iter().find(|l| l.plain.contains("code2")).unwrap();
        assert_eq!(code.spans[0].style.fg, Some(theme::CODE));
    }

    #[test]
    fn test_fenced_indent_prepended_and_plain_consistent() {
        // 边界：indent 前缀加在行首，且 plain 与可见 spans 一致。
        let lines = render_fenced_markdown("text", Style::default().fg(theme::TEXT), "  ", 80);

        assert!(lines[0].plain.starts_with("  "));
        let visible: String = lines[0]
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect();
        // markdown 行 plain 去标记后可能与 visible 不同，但普通文本两者一致。
        assert!(visible.starts_with("  "));
    }

    #[test]
    fn test_fenced_diff_block_renders_semantic_color() {
        // 正常路径：```diff 围栏内走 unified diff 渲染，新增行带新增语义色。
        let lines = render("```diff\n@@ -1 +1 @@\n-let a = 1;\n+let a = 2;\n```");
        let added = lines.iter().find(|l| l.plain.contains("2;")).unwrap();

        assert!(added
            .spans
            .iter()
            .any(|s| s.style.fg == Some(theme::DIFF_ADD_FG)));
    }
}
