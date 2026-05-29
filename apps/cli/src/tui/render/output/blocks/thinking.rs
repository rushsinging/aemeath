use crate::tui::render::output::rendered::{RenderCtx, RenderedBlock, RenderedLine};
use crate::tui::render::theme;
use crate::tui::view_model::output::TextBlockView;
use ratatui::style::Style;
use ratatui::text::Span;

pub fn render_thinking(block_id: &str, view: &TextBlockView, _ctx: &RenderCtx) -> RenderedBlock {
    let style = Style::default().fg(theme::THINKING);
    let mut lines = Vec::new();
    let mut first = true;
    for line in view.text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let prefix = if first { "💭 " } else { "   " };
        lines.push(RenderedLine::new(vec![Span::styled(
            format!("{prefix}{line}"),
            style,
        )]));
        first = false;
    }
    if lines.is_empty() {
        lines.push(RenderedLine::new(vec![Span::styled("💭 ", style)]));
    }
    RenderedBlock {
        block_id: block_id.to_string(),
        lines,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::view_model::style::SemanticStyle;

    #[test]
    fn test_thinking_prefixes_bulb_and_thinking_color() {
        let view = TextBlockView {
            key: "t".into(),
            text: "ponder".into(),
            style: SemanticStyle::Muted,
        };
        let block = render_thinking("t", &view, &RenderCtx { width: 80 });

        assert!(block.lines[0].plain.starts_with("💭"));
        assert_eq!(block.lines[0].spans[0].style.fg, Some(theme::THINKING));
    }

    #[test]
    fn test_thinking_skips_blank_lines() {
        let view = TextBlockView {
            key: "t".into(),
            text: "a\n\n\nb".into(),
            style: SemanticStyle::Muted,
        };
        let block = render_thinking("t", &view, &RenderCtx { width: 80 });

        assert!(block.lines.iter().all(|line| !line.plain.trim().is_empty()));
    }
}
