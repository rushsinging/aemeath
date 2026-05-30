use crate::tui::render::output::rendered::{RenderCtx, RenderedBlock, RenderedLine};
use crate::tui::render::theme;
use crate::tui::view_model::output::TextBlockView;
use ratatui::style::Style;
use ratatui::text::Span;

pub fn render_thinking(block_id: &str, view: &TextBlockView, _ctx: &RenderCtx) -> RenderedBlock {
    let style = Style::default().fg(theme::THINKING);
    // 💭 marker 与续行缩进现由 gutter 注入（ThinkingMessage → 💭，顶格），组件只渲染原文。
    let mut lines: Vec<RenderedLine> = view
        .text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| RenderedLine::new(vec![Span::styled(line.to_string(), style)]))
        .collect();
    if lines.is_empty() {
        lines.push(RenderedLine::default());
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
    fn test_thinking_renders_plain_text_without_self_bulb_prefix() {
        // 💭 marker 现由 gutter 注入（顶格）；组件只渲染原文，不再自拼 💭 前缀。
        let view = TextBlockView {
            key: "t".into(),
            text: "ponder".into(),
            style: SemanticStyle::Muted,
        };
        let block = render_thinking("t", &view, &RenderCtx { width: 80 });

        assert_eq!(block.lines[0].plain, "ponder");
        assert!(
            !block.lines[0].plain.contains('💭'),
            "💭 不应进入 plain（由 gutter 注入）"
        );
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
