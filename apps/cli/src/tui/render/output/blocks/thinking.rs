use crate::tui::render::output::rendered::{RenderCtx, RenderedBlock, RenderedLine};
use crate::tui::render::theme;
use crate::tui::view_model::output::TextBlockView;
use ratatui::style::Style;
use ratatui::text::Span;
use std::rc::Rc;
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
        lines: Rc::new(lines),
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
#[path = "thinking_tests.rs"]
mod tests;
