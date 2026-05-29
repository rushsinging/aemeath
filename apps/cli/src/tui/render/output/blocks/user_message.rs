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
    // 前导 `> ` marker 与续行缩进现由 gutter 注入（UserMessage → ">"），组件只渲染原文。
    let mut lines = Vec::new();
    for line in view.text.lines() {
        lines.push(RenderedLine::new(vec![Span::styled(
            line.to_string(),
            style,
        )]));
    }
    if lines.is_empty() {
        lines.push(RenderedLine::new(vec![Span::styled(String::new(), style)]));
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
    fn test_user_message_renders_raw_text_with_user_color() {
        // 前导 `> ` marker 现由 gutter 注入；组件只渲染原文，颜色仍为 USER。
        let view = TextBlockView {
            key: "u".into(),
            text: "hello".into(),
            style: SemanticStyle::Normal,
        };
        let block = render_user_message("u", &view, &RenderCtx { width: 80 });

        assert_eq!(block.lines[0].plain, "hello");
        assert_eq!(block.lines[0].spans[0].style.fg, Some(theme::USER));
    }

    #[test]
    fn test_user_message_multiline_renders_each_line_without_self_prefix() {
        let view = TextBlockView {
            key: "u".into(),
            text: "a\nb".into(),
            style: SemanticStyle::Normal,
        };
        let block = render_user_message("u", &view, &RenderCtx { width: 80 });

        assert_eq!(block.lines[0].plain, "a");
        assert_eq!(block.lines[1].plain, "b");
    }
}
