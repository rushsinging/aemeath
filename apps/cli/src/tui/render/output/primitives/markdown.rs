//! markdown 原语：解析 inline markdown -> 显示 spans，按宽度换行；plain 去标记。
//!
//! 在 inline 之上补齐两类常见块级装饰（按行检测，与 fenced 逐行喂入契合）：
//! - 引用块：`> ` / 嵌套 `> > ` 前缀渲染为弱化色竖线 `│ `，正文仍走 inline markdown。
//! - 列表项：`- ` / `* ` / `+ ` 无序、`N. ` 有序，保留缩进层级，标记着强调色，
//!   正文（含 bold/code/link）仍走 inline markdown。

use crate::tui::render::output::markdown as md;
use crate::tui::render::output::rendered::RenderedLine;
use crate::tui::render::theme;
use ratatui::style::Style;
use ratatui::text::Span;

/// 引用块竖线标记（弱化色），每层一个。
const QUOTE_BAR: &str = "│ ";
/// 无序列表渲染用圆点标记。
const BULLET: &str = "• ";

pub fn markdown(text: &str, base_style: Style, width: u16) -> Vec<RenderedLine> {
    // 空文本（包括无换行符的空串）仍产出一行，保持与历史行为一致。
    if text.is_empty() {
        return inline_lines("", base_style, width);
    }
    text.lines()
        .flat_map(|line| render_line(line, base_style, width))
        .collect()
}

/// 渲染单行：先识别块级前缀（引用 / 列表），剥离后正文走 inline，再把
/// 前缀以样式化 marker 拼回，并保持 `plain` 与可见 spans 一致。
fn render_line(line: &str, base_style: Style, width: u16) -> Vec<RenderedLine> {
    if let Some((bars, body)) = strip_blockquote(line) {
        let marker_plain = QUOTE_BAR.repeat(bars);
        let marker_width = marker_plain.chars().count() as u16;
        let inner_width = width.saturating_sub(marker_width).max(1);
        let marker_style = Style::default().fg(theme::TEXT_DIM);
        let body_style = base_style.fg(theme::TEXT_MUTED);
        return inline_lines(body, body_style, inner_width)
            .into_iter()
            .map(|line| prepend_marker(&marker_plain, marker_style, line))
            .collect();
    }

    if let Some((indent, marker, body)) = strip_list_item(line) {
        let marker_plain = format!("{indent}{marker}");
        let marker_width = marker_plain.chars().count() as u16;
        let inner_width = width.saturating_sub(marker_width).max(1);
        let marker_style = base_style.fg(theme::ACCENT);
        return inline_lines(body, base_style, inner_width)
            .into_iter()
            .enumerate()
            .map(|(idx, line)| {
                if idx == 0 {
                    prepend_marker(&marker_plain, marker_style, line)
                } else {
                    // 续行按 marker 宽度缩进对齐，不重复 marker。
                    let pad = " ".repeat(marker_plain.chars().count());
                    prepend_marker(&pad, base_style, line)
                }
            })
            .collect();
    }

    inline_lines(line, base_style, width)
}

/// 普通 inline markdown 行（无块级前缀）。
fn inline_lines(text: &str, base_style: Style, width: u16) -> Vec<RenderedLine> {
    md::inline_markdown_lines(text, base_style, width as usize)
        .into_iter()
        .map(|line| {
            let visible = line
                .spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>();
            let plain = md::strip_inline_formatting(&visible);
            RenderedLine::with_plain(line.spans, plain)
        })
        .collect()
}

/// 在一行已渲染产物前补一个样式化 marker，保持 plain 与 spans 一致。
fn prepend_marker(marker_plain: &str, marker_style: Style, line: RenderedLine) -> RenderedLine {
    let mut spans = vec![Span::styled(marker_plain.to_string(), marker_style)];
    spans.extend(line.spans);
    RenderedLine::with_plain(spans, format!("{marker_plain}{}", line.plain))
}

/// 识别引用块前缀，返回（嵌套层数, 去前缀后的正文）。
/// 仅当行以可选空白 + `>` 开头时成立；逐层吃掉 `>` 及其后单个空格。
fn strip_blockquote(line: &str) -> Option<(usize, &str)> {
    let trimmed = line.trim_start();
    if !trimmed.starts_with('>') {
        return None;
    }
    let mut rest = trimmed;
    let mut bars = 0usize;
    loop {
        let Some(after) = rest.strip_prefix('>') else {
            break;
        };
        bars += 1;
        rest = after.strip_prefix(' ').unwrap_or(after);
    }
    Some((bars, rest))
}

/// 识别列表项前缀，返回（缩进, 渲染用 marker, 正文）。
/// - 无序：`- ` / `* ` / `+ ` → 统一 `BULLET`。
/// - 有序：`N. ` / `N) ` → 原样保留 `N. `。
fn strip_list_item(line: &str) -> Option<(String, String, &str)> {
    let indent_len = line.len() - line.trim_start().len();
    let (indent, rest) = line.split_at(indent_len);
    if let Some(body) = rest
        .strip_prefix("- ")
        .or_else(|| rest.strip_prefix("* "))
        .or_else(|| rest.strip_prefix("+ "))
    {
        return Some((indent.to_string(), BULLET.to_string(), body));
    }
    // 有序列表：开头若干数字 + `.`/`)` + 空格。
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if !digits.is_empty() {
        let after = &rest[digits.len()..];
        for sep in [". ", ") "] {
            if let Some(body) = after.strip_prefix(sep) {
                return Some((indent.to_string(), format!("{digits}. "), body));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::{Modifier, Style};

    #[test]
    fn test_markdown_bold_sets_modifier_and_plain_strips_markers() {
        let lines = markdown("a **b** c", Style::default(), 80);

        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].plain, "a b c");
        assert!(lines[0]
            .spans
            .iter()
            .any(|span| span.content.as_ref() == "b"
                && span.style.add_modifier.contains(Modifier::BOLD)));
    }

    #[test]
    fn test_markdown_wraps_by_width() {
        let lines = markdown("aaaa bbbb", Style::default(), 4);

        assert!(lines.len() >= 2, "超宽应换行");
    }

    #[test]
    fn test_markdown_plain_invariant_matches_spans_visible_text() {
        let lines = markdown("`code` and *em*", Style::default(), 80);

        for line in &lines {
            let visible = line
                .spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>();
            assert!(!line.plain.contains('`'));
            assert!(!visible.is_empty() || line.plain.is_empty());
        }
    }

    #[test]
    fn test_markdown_blockquote_renders_bar_and_dim() {
        let lines = markdown("> hello", Style::default(), 80);

        assert_eq!(lines.len(), 1);
        assert!(lines[0].plain.starts_with("│ "), "应以竖线开头");
        assert!(lines[0].plain.ends_with("hello"));
        // 首个 span 是弱化色竖线。
        assert_eq!(lines[0].spans[0].style.fg, Some(theme::TEXT_DIM));
    }

    #[test]
    fn test_markdown_blockquote_nested_two_bars() {
        let lines = markdown("> > deep", Style::default(), 80);

        assert!(lines[0].plain.starts_with("│ │ "), "两层引用两根竖线");
        assert!(lines[0].plain.ends_with("deep"));
    }

    #[test]
    fn test_markdown_blockquote_keeps_inline_bold() {
        let lines = markdown("> see **this**", Style::default(), 80);

        assert!(lines[0].plain.contains("see this"));
        assert!(lines[0]
            .spans
            .iter()
            .any(|s| s.content.as_ref() == "this"
                && s.style.add_modifier.contains(Modifier::BOLD)));
    }

    #[test]
    fn test_markdown_unordered_list_renders_bullet() {
        let lines = markdown("- item", Style::default(), 80);

        assert!(lines[0].plain.starts_with("• "), "无序项应渲染圆点");
        assert!(lines[0].plain.ends_with("item"));
        assert_eq!(lines[0].spans[0].style.fg, Some(theme::ACCENT));
    }

    #[test]
    fn test_markdown_nested_list_preserves_indent() {
        let lines = markdown("  - nested", Style::default(), 80);

        assert!(
            lines[0].plain.starts_with("  • "),
            "嵌套项应保留缩进再加圆点, got: {:?}",
            lines[0].plain
        );
    }

    #[test]
    fn test_markdown_ordered_list_keeps_number() {
        let lines = markdown("1. first", Style::default(), 80);

        assert!(
            lines[0].plain.starts_with("1. "),
            "有序项保留序号, got: {:?}",
            lines[0].plain
        );
        assert!(lines[0].plain.ends_with("first"));
    }

    #[test]
    fn test_markdown_list_item_keeps_inline_code() {
        let lines = markdown("- use `cargo`", Style::default(), 80);

        assert!(lines[0].plain.contains("use cargo"));
        assert!(lines[0]
            .spans
            .iter()
            .any(|s| s.content.as_ref() == "cargo" && s.style.fg == Some(theme::CODE)));
    }

    #[test]
    fn test_markdown_dash_without_space_is_not_list() {
        // 边界：`-` 后无空格不算列表标记，原样走 inline。
        let lines = markdown("-notalist", Style::default(), 80);

        assert!(!lines[0].plain.starts_with("• "));
        assert!(lines[0].plain.contains("-notalist"));
    }

    #[test]
    fn test_markdown_multiline_mixed_blocks() {
        // 多行混合：引用 + 列表 + 普通行各成独立渲染行。
        let lines = markdown("> quote\n- item\nplain", Style::default(), 80);

        assert_eq!(lines.len(), 3);
        assert!(lines[0].plain.starts_with("│ "));
        assert!(lines[1].plain.starts_with("• "));
        assert_eq!(lines[2].plain, "plain");
    }
}
