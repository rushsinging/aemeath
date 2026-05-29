use crate::tui::render::output::rendered::{RenderCtx, RenderedBlock, RenderedLine};
use crate::tui::render::theme;
use crate::tui::view_model::output::TextBlockView;
use ratatui::style::Style;
use ratatui::text::Span;

pub fn render_user_message(
    block_id: &str,
    view: &TextBlockView,
    _ctx: &RenderCtx,
) -> RenderedBlock {
    let style = Style::default().fg(theme::USER);
    let mut lines = Vec::new();
    for (idx, line) in view.text.lines().enumerate() {
        let prefix = if idx == 0 { "> " } else { "  " };
        lines.push(RenderedLine::new(vec![Span::styled(
            format!("{prefix}{line}"),
            style,
        )]));
    }
    if lines.is_empty() {
        lines.push(RenderedLine::new(vec![Span::styled("> ", style)]));
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
    fn test_user_message_prefixes_gt_and_uses_user_color() {
        let view = TextBlockView {
            key: "u".into(),
            text: "hello".into(),
            style: SemanticStyle::Normal,
        };
        let block = render_user_message("u", &view, &RenderCtx { width: 80 });

        assert_eq!(block.lines[0].plain, "> hello");
        assert_eq!(block.lines[0].spans[0].style.fg, Some(theme::USER));
    }

    #[test]
    fn test_user_message_multiline_indents_continuation() {
        let view = TextBlockView {
            key: "u".into(),
            text: "a\nb".into(),
            style: SemanticStyle::Normal,
        };
        let block = render_user_message("u", &view, &RenderCtx { width: 80 });

        assert_eq!(block.lines[0].plain, "> a");
        assert_eq!(block.lines[1].plain, "  b");
    }
}
