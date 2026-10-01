use crate::tui::render::output::primitives::fenced::render_fenced_markdown;
use crate::tui::render::output::rendered::{RenderCtx, RenderedBlock, RenderedLine};
use crate::tui::render::theme;
use crate::tui::view_model::output::TextBlockView;
use ratatui::style::Style;
use std::rc::Rc;

pub fn render_assistant_message(
    block_id: &str,
    view: &TextBlockView,
    ctx: &RenderCtx,
) -> RenderedBlock {
    let base = Style::default().fg(theme::ASSISTANT);
    // fence/markdown/table 解析统一走 primitives::fenced（DRY，与工具结果共用）。
    let mut lines = render_fenced_markdown(&view.text, base, ctx.text_width, &ctx.markdown_spacing);

    if lines.is_empty() {
        lines.push(RenderedLine::default());
    }
    RenderedBlock {
        block_id: block_id.to_string(),
        lines: Rc::new(lines),
    }
}

#[cfg(test)]
#[path = "assistant_message_tests.rs"]
mod tests;
