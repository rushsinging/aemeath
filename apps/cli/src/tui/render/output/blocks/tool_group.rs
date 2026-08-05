use crate::tui::render::output::blocks::diagnostic::semantic_color;
use crate::tui::render::output::rendered::{RenderCtx, RenderedBlock, RenderedLine};
use crate::tui::view_model::output::ToolGroupBlockView;
use ratatui::style::Style;
use ratatui::text::Span;
use std::rc::Rc;

pub fn render_tool_group(
    block_id: &str,
    view: &ToolGroupBlockView,
    _ctx: &RenderCtx,
) -> RenderedBlock {
    let title = format!("── {} ──", view.title);
    RenderedBlock {
        block_id: block_id.to_string(),
        lines: Rc::new(vec![RenderedLine::new(vec![Span::styled(
            title,
            Style::default().fg(semantic_color(view.style)),
        )])]),
    }
}

#[cfg(test)]
#[path = "tool_group_tests.rs"]
mod tests;
