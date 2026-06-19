use crate::tui::render::output::rendered::{RenderCtx, RenderedBlock, RenderedLine};
use crate::tui::render::theme;
use crate::tui::view_model::output::TextBlockView;
use ratatui::style::Style;
use ratatui::text::Span;
use unicode_width::UnicodeWidthChar;

pub fn render_thinking(block_id: &str, view: &TextBlockView, ctx: &RenderCtx) -> RenderedBlock {
    let style = Style::default().fg(theme::THINKING);
    // 💭 marker 与续行缩进现由 gutter 注入（ThinkingMessage → 💭，顶格），组件只渲染原文。
    let mut lines: Vec<RenderedLine> = view
        .text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .flat_map(|line| render_wrapped_thinking_line(line, style, ctx.text_width))
        .collect();
    if lines.is_empty() {
        lines.push(RenderedLine::default());
    }
    RenderedBlock {
        block_id: block_id.to_string(),
        lines,
    }
}

fn render_wrapped_thinking_line(line: &str, style: Style, width: u16) -> Vec<RenderedLine> {
    let max_width = width as usize;
    if max_width == 0 {
        return vec![plain_thinking_line(line, style)];
    }

    let mut rendered = Vec::new();
    let mut current = String::new();
    let mut current_width = 0usize;

    for ch in line.chars() {
        let ch_width = ch.width().unwrap_or(1);
        if !current.is_empty() && current_width + ch_width > max_width {
            rendered.push(plain_thinking_line(&current, style));
            current.clear();
            current_width = 0;
        }
        current.push(ch);
        current_width += ch_width;
    }

    if !current.is_empty() || rendered.is_empty() {
        rendered.push(plain_thinking_line(&current, style));
    }
    rendered
}

fn plain_thinking_line(text: &str, style: Style) -> RenderedLine {
    RenderedLine::new(vec![Span::styled(text.to_string(), style)])
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
        let block = render_thinking("t", &view, &RenderCtx { text_width: 80 });

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
        let block = render_thinking("t", &view, &RenderCtx { text_width: 80 });

        assert!(block.lines.iter().all(|line| !line.plain.trim().is_empty()));
    }

    #[test]
    fn test_thinking_wraps_long_reasoning_line_to_render_width() {
        let text = "The user is greeting me in Chinese. According to the system reminder, I should use Chinese for thinking and response.";
        let view = TextBlockView {
            key: "t".into(),
            text: text.into(),
            style: SemanticStyle::Muted,
        };
        let block = render_thinking("t", &view, &RenderCtx { text_width: 16 });

        assert!(block.lines.len() > 1, "长 reasoning 行应按渲染宽度拆成多行");
        assert!(block
            .lines
            .iter()
            .all(|line| line.plain.chars().count() <= 16));
        let combined_plain = block
            .lines
            .iter()
            .map(|line| line.plain.as_str())
            .collect::<String>();
        assert_eq!(combined_plain, text);
        assert!(block
            .lines
            .iter()
            .flat_map(|line| line.spans.iter())
            .all(|span| span.style.fg == Some(theme::THINKING)));
    }

    #[test]
    fn test_thinking_renders_markdown_markers_as_plain_text() {
        let text = "# plan: keep **stars**, _underscores_, `ticks`, and *asterisks*";
        let view = TextBlockView {
            key: "t".into(),
            text: text.into(),
            style: SemanticStyle::Muted,
        };
        let block = render_thinking("t", &view, &RenderCtx { text_width: 120 });

        assert_eq!(block.lines.len(), 1);
        assert_eq!(block.lines[0].plain, text);
        assert_eq!(block.lines[0].spans.len(), 1);
        assert_eq!(block.lines[0].spans[0].content.as_ref(), text);
        assert_eq!(block.lines[0].spans[0].style.fg, Some(theme::THINKING));
    }
}
