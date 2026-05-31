use crate::tui::render::output::rendered::{RenderCtx, RenderedBlock, RenderedLine};
use crate::tui::render::theme;
use crate::tui::view_model::output::TextBlockView;
use ratatui::style::Style;
use ratatui::text::Span;

pub fn render_queued_submission(
    block_id: &str,
    _view: &TextBlockView,
    _ctx: &RenderCtx,
) -> RenderedBlock {
    let style = Style::default().fg(theme::TEXT_DIM);
    let mut lines = Vec::new();
    lines.push(RenderedLine::new(vec![Span::styled(
        ">".to_string(),
        style,
    )]));
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
    fn test_queued_submission_marks_and_dims() {
        let view = TextBlockView {
            key: "q".into(),
            text: "draft".into(),
            style: SemanticStyle::Muted,
        };
        let block = render_queued_submission("q", &view, &RenderCtx { width: 80 });

        assert_eq!(block.lines.len(), 1);
        assert_eq!(block.lines[0].plain, ">");
        assert_eq!(block.lines[0].spans[0].style.fg, Some(theme::TEXT_DIM));
    }
}
