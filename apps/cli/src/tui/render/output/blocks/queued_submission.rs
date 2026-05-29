use crate::tui::render::output::rendered::{RenderCtx, RenderedBlock, RenderedLine};
use crate::tui::render::theme;
use crate::tui::view_model::output::TextBlockView;
use ratatui::style::Style;
use ratatui::text::Span;

pub fn render_queued_submission(
    block_id: &str,
    view: &TextBlockView,
    _ctx: &RenderCtx,
) -> RenderedBlock {
    let style = Style::default().fg(theme::TEXT_DIM);
    let mut lines = Vec::new();
    for (idx, line) in view.text.lines().enumerate() {
        let text = if idx == 0 {
            format!("⏳ 排队中: {line}")
        } else {
            format!("   {line}")
        };
        lines.push(RenderedLine::new(vec![Span::styled(text, style)]));
    }
    if lines.is_empty() {
        lines.push(RenderedLine::new(vec![Span::styled("⏳ 排队中: ", style)]));
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
    fn test_queued_submission_marks_and_dims() {
        let view = TextBlockView {
            key: "q".into(),
            text: "draft".into(),
            style: SemanticStyle::Muted,
        };
        let block = render_queued_submission("q", &view, &RenderCtx { width: 80 });

        assert!(block.lines[0].plain.contains("draft"));
        assert!(block.lines[0].plain.contains("排队中"));
        assert_eq!(block.lines[0].spans[0].style.fg, Some(theme::TEXT_DIM));
    }
}
