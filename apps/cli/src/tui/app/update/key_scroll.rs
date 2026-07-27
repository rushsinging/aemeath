use crate::tui::app::App;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

pub(super) fn handle_scroll_key(app: &mut App, key: KeyEvent, modifiers: KeyModifiers) -> bool {
    // 滚动真相归 view_state；widget 镜像由每帧 `refresh_output_scroll_from_view_state` 写回。
    // 总行数由 widget 的 document 提供（view_state 不持有 document）。
    let total_lines = app.output_area.document().total_lines();
    let view = &mut app.view_state.output;
    let before_limit = view.render_line_limit();
    let before_offset = view.scroll_offset;
    let before_auto_scroll = view.auto_scroll;
    let mut expanded = false;
    match (modifiers, key.code) {
        (KeyModifiers::NONE, KeyCode::PageUp) => {
            view.scroll_up(10, total_lines);
            expanded = view.try_load_older_at_top(total_lines);
        }
        (KeyModifiers::NONE, KeyCode::PageDown) => view.scroll_down(10),
        (KeyModifiers::SHIFT, KeyCode::Up) => {
            view.scroll_up(1, total_lines);
            expanded = view.try_load_older_at_top(total_lines);
        }
        (KeyModifiers::SHIFT, KeyCode::Down) => view.scroll_down(1),
        (KeyModifiers::SHIFT, KeyCode::Home) => {
            expanded = view.scroll_to_top(total_lines);
        }
        (KeyModifiers::SHIFT, KeyCode::End) => view.scroll_to_bottom(),
        _ => return false,
    }
    if expanded {
        crate::tui::log_debug!(
            "tui.history.load_older expanded=true key={:?} modifiers={:?} total_lines={} before_limit={} after_limit={} before_offset={} after_offset={} before_auto_scroll={} after_auto_scroll={}",
            key.code,
            modifiers,
            total_lines,
            before_limit,
            view.render_line_limit(),
            before_offset,
            view.scroll_offset,
            before_auto_scroll,
            view.auto_scroll
        );
        app.mark_output_dirty();
    } else {
        crate::tui::log_trace!(
            "tui.history.load_older expanded=false key={:?} modifiers={:?} total_lines={} visible_height={} limit={} offset={} auto_scroll={} source_total_lines={} pending_load_older={}",
            key.code,
            modifiers,
            total_lines,
            view.last_visible_height,
            view.render_line_limit(),
            view.scroll_offset,
            view.auto_scroll,
            view.source_total_lines,
            view.pending_load_older
        );
    }
    true
}
