use crate::tui::render::output::rendered::{RenderCtx, RenderedBlock, RenderedLine};
use crate::tui::render::theme;
use crate::tui::view_model::output::ToolGroupBlockView;
use ratatui::style::Style;
use ratatui::text::Span;
use std::rc::Rc;

pub fn render_tool_group(
    block_id: &str,
    view: &ToolGroupBlockView,
    _ctx: &RenderCtx,
) -> RenderedBlock {
    RenderedBlock {
        block_id: block_id.to_string(),
        lines: Rc::new(vec![RenderedLine::new(vec![Span::styled(
            view.title.clone(),
            Style::default().fg(theme::ACCENT_BRIGHT),
        )])
        .with_style(Style::default().fg(theme::TEXT))]),
    }
}

#[cfg(test)]
#[path = "tool_group_tests.rs"]
mod tests;
