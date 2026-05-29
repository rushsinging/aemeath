//! markdown 原语：解析 inline markdown -> 显示 spans，按宽度换行；plain 去标记。

use crate::tui::render::output::markdown as md;
use crate::tui::render::output::rendered::RenderedLine;
use ratatui::style::Style;

pub fn markdown(text: &str, base_style: Style, width: u16) -> Vec<RenderedLine> {
    let lines = md::inline_markdown_lines(text, base_style, width as usize);
    lines
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
            assert_eq!(
                visible,
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            );
            assert!(!line.plain.contains('`'));
        }
    }
}
