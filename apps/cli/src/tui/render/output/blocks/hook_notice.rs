use crate::tui::render::output::primitives::wrap::wrap_styled_text;
use crate::tui::render::output::rendered::{RenderCtx, RenderedBlock};
use crate::tui::render::theme;
use crate::tui::view_model::output::HookNoticeBlockView;
use ratatui::style::Style;
use std::rc::Rc;

pub fn render_hook_notice(
    block_id: &str,
    view: &HookNoticeBlockView,
    ctx: &RenderCtx,
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
    let max_width = usize::from(ctx.text_width);
    let mut lines = wrap_styled_text(&view.title, title_style, max_width);
    lines.extend(wrap_styled_text(&view.body, body_style, max_width));
    RenderedBlock {
        block_id: block_id.to_string(),
        lines: Rc::new(lines),
    }
}
