use crate::tui::render::output::primitives::wrap::{wrap_spans_with_prefix, WrapMode};
use crate::tui::render::output::rendered::{RenderCtx, RenderedBlock, RenderedLine};
use crate::tui::render::theme;
use crate::tui::view_model::output::TextBlockView;
use ratatui::style::Style;
use ratatui::text::Span;
use std::rc::Rc;

pub fn render_user_message(block_id: &str, view: &TextBlockView, ctx: &RenderCtx) -> RenderedBlock {
    let style = Style::default().fg(theme::USER).bg(theme::USER_BG);
    // 前导 `> ` marker 与续行缩进现由 gutter 注入（UserMessage → ">"），组件只渲染原文。
    let mut lines = Vec::new();
    for line in view.text.lines() {
        if line.is_empty() {
            lines.push(RenderedLine::empty());
            continue;
        }
        lines.extend(wrap_spans_with_prefix(
            vec![Span::styled(line.to_string(), style)],
            ctx.text_width as usize,
            None,
            WrapMode::Word,
        ));
    }
    if lines.is_empty() {
        lines.push(RenderedLine::empty());
    }
    RenderedBlock {
        block_id: block_id.to_string(),
        lines: Rc::new(lines),
    }
}

#[cfg(test)]
#[path = "user_message_tests.rs"]
mod tests;
