use crate::tui::render::output::rendered::{RenderCtx, RenderedBlock, RenderedLine};
use crate::tui::render::theme;
use crate::tui::view_model::output::HookNoticeBlockView;
use ratatui::style::Style;
use ratatui::text::Span;
use std::rc::Rc;

pub fn render_hook_notice(
    block_id: &str,
    view: &HookNoticeBlockView,
    _ctx: &RenderCtx,
) -> RenderedBlock {
    let (title_color, body_color) = match view.kind {
        crate::tui::adapter::runtime_view::TuiHookNoticeKind::Blocked
        | crate::tui::adapter::runtime_view::TuiHookNoticeKind::Failed => {
            (theme::ERROR, theme::TEXT_MUTED)
        }
        crate::tui::adapter::runtime_view::TuiHookNoticeKind::Info => {
            (theme::TEXT_MUTED, theme::TEXT)
        }
    };
    let title_style = Style::default().fg(title_color);
    let body_style = Style::default().fg(body_color);
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
