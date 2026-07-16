//! fenced markdown 原语：解析含 fenced code block 的多行文本为渲染行。
//!
//! 状态机（in_fence / fence_lang）是函数局部状态，随调用结束销毁，
//! 因此天然隔离跨调用的 code 样式泄漏（修 #65 的结构基础）。
//!
//! 行为：
//! - ``` ``` `` 围栏行：切换 fence 状态，普通代码 fence 按 TEXT_DIM 着色围栏标记本身。
//! - fence 内：``` ```text `` 隐藏围栏并按普通 Markdown 渲染内容；``` ```diff `` 走 unified diff 渲染；
//!   其他 fence 按 fence_lang 语法高亮，无语言信息时按 CODE 单色。
//! - fence 外：表格走 table 原语，普通行走 inline markdown。
//!
//! 产出 depth=0、无缩进的行（spans 与 plain 均不含前导缩进）。
//! 缩进 / gutter 由上层渲染器（Phase 4）统一注入，#60 决策。

use crate::tui::render::output::markdown::{is_table_row, is_table_separator};
use crate::tui::render::output::primitives::{
    markdown::{markdown, should_skip_blank_outside_fence},
    table::table,
    unified_diff::render_unified_diff,
    wrap::wrap_spans_to_rendered_lines,
};
use crate::tui::render::output::rendered::RenderedLine;
use crate::tui::render::{syntax, theme};
use ratatui::style::Style;
use ratatui::text::Span;

/// 解析含 fenced code block 的多行文本为渲染行。
///
/// - `text`：多行原始文本。
/// - `base_style`：fence 外普通文本的基色（assistant 用 ASSISTANT，工具结果用结果色）。
/// - `width`：可用宽度，传给 markdown / table 做换行。
pub fn render_fenced_markdown(text: &str, base_style: Style, width: u16) -> Vec<RenderedLine> {
    let src = text.lines().collect::<Vec<_>>();
    let mut lines: Vec<RenderedLine> = Vec::new();
    let mut idx = 0;
    let mut in_fence = false;
    let mut fence_lang: Option<String> = None;
    let mut prev_blank_outside = true; // 跳过开头空行

    while idx < src.len() {
        let Some(line) = src.get(idx) else {
            break;
        };
        let trimmed = line.trim_start();
        if is_fence_marker(trimmed) {
            // fence 标记行不参与空行合并
            prev_blank_outside = false;
            if in_fence {
                let should_show_marker = fence_lang.as_deref() != Some("text");
                in_fence = false;
                fence_lang = None;
                if should_show_marker {
                    lines.push(RenderedLine::new(vec![Span::styled(
                        line.to_string(),
                        Style::default().fg(theme::TEXT_DIM),
                    )]));
                }
            } else {
                let lang = fence_language(trimmed);
                let should_show_marker = lang.as_deref() != Some("text");
                in_fence = true;
                fence_lang = lang;
                if should_show_marker {
                    lines.push(RenderedLine::new(vec![Span::styled(
                        line.to_string(),
                        Style::default().fg(theme::TEXT_DIM),
                    )]));
                }
            }
            idx += 1;
            continue;
        }

        if in_fence {
            // ` ```text ` 作为 Markdown 文本容器：隐藏围栏，内容按普通 Markdown 渲染。
            if fence_lang.as_deref() == Some("text") {
                lines.extend(markdown(line, base_style, width));
                idx += 1;
                continue;
            }

            // ` ```diff ` 代码块走 unified diff 渲染（行号信息来自 @@ 原文）。
            if fence_lang.as_deref() == Some("diff") {
                lines.extend(render_unified_diff(line, None, width));
                idx += 1;
                continue;
            }
            let syntax_ref = fence_lang
                .as_deref()
                .and_then(syntax::language_by_fence_info);
            if let Some(parts) = syntax::highlight_line(line, syntax_ref.as_ref()) {
                lines.extend(wrap_spans_to_rendered_lines(
                    crate::tui::render::output::primitives::spanparts_to_spans(&parts),
                    width as usize,
                ));
            } else {
                lines.extend(wrap_spans_to_rendered_lines(
                    vec![Span::styled(
                        line.to_string(),
                        Style::default().fg(theme::CODE),
                    )],
                    width as usize,
                ));
            }
            idx += 1;
            continue;
        }

        // fence 外：跳过连续空行（紧凑模式）
        if should_skip_blank_outside_fence(line, &mut prev_blank_outside) {
            idx += 1;
            continue;
        }

        if width >= crate::tui::render::output::gutter::NARROW_DISABLE_TABLE_THRESHOLD
            && is_table_row(line)
            && src
                .get(idx + 1)
                .map(|next| is_table_separator(next))
                .unwrap_or(false)
        {
            // 表格块含表头、分隔行与全部数据行：分隔行不是 is_table_row（被排除），
            // 故收集时需同时容纳 is_table_separator，否则块只含表头、其余行原样泄漏。
            let mut end = idx;
            while end < src.len()
                && src
                    .get(end)
                    .map(|candidate| is_table_row(candidate) || is_table_separator(candidate))
                    .unwrap_or(false)
            {
                end += 1;
            }
            let block_src: Vec<&str> = src.iter().skip(idx).take(end - idx).copied().collect();
            lines.extend(table(&block_src, base_style, width));
            idx = end;
            continue;
        }

        lines.extend(markdown(line, base_style, width));
        idx += 1;
    }

    lines
}

fn is_fence_marker(trimmed: &str) -> bool {
    trimmed.starts_with("```")
}

fn fence_language(trimmed: &str) -> Option<String> {
    let lang = trimmed.trim_start_matches('`').trim();
    if lang.is_empty() {
        None
    } else {
        Some(lang.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(text: &str) -> Vec<RenderedLine> {
        render_fenced_markdown(text, Style::default().fg(theme::TEXT), 80)
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
    fn test_fenced_rust_language_name_uses_syntect_highlight() {
        let lines = render("```rust\nfn main() {\n``` ");
        let code = lines.iter().find(|l| l.plain.contains("fn main")).unwrap();

        assert!(
            code.spans.len() > 1,
            "rust fence 应使用 syntect 语法高亮拆分多个 span，而不是 CODE 单色，got: {:?}",
            code.spans
        );
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
    fn test_fenced_text_block_renders_inner_markdown_without_fence_lines() {
        use ratatui::style::Modifier;

        let lines = render("```text\n**bold**\n- item\n```");
        let plains: Vec<&str> = lines.iter().map(|line| line.plain.as_str()).collect();

        assert_eq!(plains, vec!["bold", "• item"]);
        assert!(
            lines[0]
                .spans
                .iter()
                .any(|span| span.content.as_ref() == "bold"
                    && span.style.add_modifier.contains(Modifier::BOLD)),
            "text fence 内容应按 markdown 渲染 bold，got: {:?}",
            lines[0].spans
        );
        assert_eq!(lines[1].spans[0].style.fg, Some(theme::ACCENT));
        assert!(
            !plains.iter().any(|plain| plain.contains("```")),
            "text fence 不应显示围栏行，got: {plains:?}"
        );
    }

    #[test]
    fn test_fenced_unlabeled_code_block_still_renders_fence_and_code() {
        let lines = render("```\n**not markdown**\n```");
        let plains: Vec<&str> = lines.iter().map(|line| line.plain.as_str()).collect();

        assert_eq!(plains, vec!["```", "**not markdown**", "```"]);
        assert_eq!(lines[1].spans[0].style.fg, Some(theme::CODE));
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
    fn test_render_fenced_markdown_no_indent_in_plain_or_spans() {
        // #60：原语产出 depth=0、无缩进行——plain 与可见 spans 均不含前导缩进。
        let lines = render_fenced_markdown("hello\nworld", Style::default(), 80);
        assert!(!lines[0].plain.starts_with(' '), "plain 不应含前导缩进");
        let first_span = lines[0].spans[0].content.as_ref();
        assert!(!first_span.starts_with(' '), "spans 不应含前导缩进");
    }

    #[test]
    fn test_table_block_renders_separator_and_all_data_rows() {
        // 回归（正文表格只渲染表头 bug）：表头+分隔+多数据行整块都应成表格，
        // 不得把分隔行/数据行当普通文本原样输出。
        let text = "## 活跃 Bug\n\n| # | 标题 | 状态 |\n|---|------|------|\n| 49 | aaa | 修复中 |\n| 54 | bbb | 待确认 |";
        let lines = render(text);
        let plains: Vec<String> = lines.iter().map(|l| l.plain.clone()).collect();

        // 原始 ASCII 分隔行不得出现（应被表格渲染消费）。
        assert!(
            !plains.iter().any(|p| p.contains("|---")),
            "分隔行不应原样输出, got: {plains:?}"
        );
        // 每个数据行都应被渲染进表格单元（所在行带 │），而非原样 | 文本。
        for marker in ["49", "54"] {
            let row = plains
                .iter()
                .find(|p| p.contains(marker))
                .unwrap_or_else(|| panic!("缺数据行 {marker}, got: {plains:?}"));
            assert!(
                row.contains('│'),
                "数据行 {marker} 应渲染为表格单元(带 │), got: {row:?}"
            );
            assert!(
                !row.contains(" | "),
                "数据行 {marker} 不应残留原始 ASCII | 分隔, got: {row:?}"
            );
        }
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
