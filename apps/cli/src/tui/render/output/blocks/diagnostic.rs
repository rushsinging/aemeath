use crate::tui::render::output::rendered::{RenderCtx, RenderedBlock, RenderedLine};
use crate::tui::render::theme;
use crate::tui::view_model::output::TextBlockView;
use crate::tui::view_model::style::SemanticStyle;
use ratatui::style::{Color, Style};
use ratatui::text::Span;

pub fn semantic_color(style: SemanticStyle) -> Color {
    match style {
        SemanticStyle::Normal => theme::TEXT,
        SemanticStyle::Muted => theme::TEXT_MUTED,
        SemanticStyle::Running => theme::TOOL_RUNNING,
        SemanticStyle::Success => theme::SUCCESS,
        SemanticStyle::Error => theme::ERROR,
        SemanticStyle::Warning => theme::WARNING,
        SemanticStyle::Accent => theme::ACCENT,
    }
}

pub fn render_diagnostic(block_id: &str, view: &TextBlockView, _ctx: &RenderCtx) -> RenderedBlock {
    let style = Style::default().fg(semantic_color(view.style));
    let lines = view
        .text
        .lines()
        .map(|line| RenderedLine::new(vec![Span::styled(line.to_string(), style)]))
        .collect();
    RenderedBlock {
        block_id: block_id.to_string(),
        lines,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::render::output::blocks::separator::render_separator;

    #[test]
    fn test_diagnostic_error_uses_error_color() {
        let view = TextBlockView {
            key: "e".into(),
            text: "boom".into(),
            style: SemanticStyle::Error,
        };
        let block = render_diagnostic("e", &view, &RenderCtx { width: 80 });

        assert_eq!(block.lines[0].plain, "boom");
        assert_eq!(block.lines[0].spans[0].style.fg, Some(theme::ERROR));
    }

    #[test]
    fn test_separator_emits_blank_line() {
        let block = render_separator("sep-0");

        assert_eq!(block.lines.len(), 1);
        assert_eq!(block.lines[0].plain, "");
    }
}
