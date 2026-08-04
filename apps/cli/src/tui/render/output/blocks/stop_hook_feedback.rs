use crate::tui::render::output::rendered::{RenderCtx, RenderedBlock, RenderedLine};
use crate::tui::render::theme;
use crate::tui::view_model::output::StopHookFeedbackBlockView;
use ratatui::style::Style;
use ratatui::text::Span;
use std::rc::Rc;

pub fn render_stop_hook_feedback(
    block_id: &str,
    view: &StopHookFeedbackBlockView,
    _ctx: &RenderCtx,
) -> RenderedBlock {
    let title_style = Style::default().fg(theme::ERROR);
    let body_style = Style::default().fg(theme::TEXT_MUTED);
    let mut lines = vec![RenderedLine::new(vec![Span::styled(
        view.title.clone(),
        title_style,
    )])];
    lines.extend(
        view.body
            .lines()
            .map(|line| RenderedLine::new(vec![Span::styled(line.to_string(), body_style)])),
    );
    RenderedBlock {
        block_id: block_id.to_string(),
        lines: Rc::new(lines),
    }
}
