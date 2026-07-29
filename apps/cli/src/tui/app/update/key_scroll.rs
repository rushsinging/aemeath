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
    let before_tail_offset = view.history_window_tail_offset;
    let (action, window_changed) = match (modifiers, key.code) {
        (KeyModifiers::NONE, KeyCode::PageUp) => {
            view.scroll_up(10, total_lines);
            ("page_up", view.try_load_older_near_top(total_lines))
        }
        (KeyModifiers::NONE, KeyCode::PageDown) => ("page_down", view.scroll_down(10)),
        (KeyModifiers::SHIFT, KeyCode::Up) => {
            view.scroll_up(1, total_lines);
            ("line_up", view.try_load_older_near_top(total_lines))
        }
        (KeyModifiers::SHIFT, KeyCode::Down) => ("line_down", view.scroll_down(1)),
        (KeyModifiers::SHIFT, KeyCode::Home) => ("top", view.scroll_to_top(total_lines)),
        (KeyModifiers::SHIFT, KeyCode::End) => {
            view.scroll_to_bottom();
            (
                "bottom",
                before_tail_offset > 0 || before_limit != view.render_line_limit(),
            )
        }
        _ => return false,
    };
    crate::tui::log_debug!(
        "tui.output.scroll_input source=keyboard action={} key={:?} modifiers={:?} total_lines={} visible_height={} window_changed={} before_limit={} after_limit={} before_offset={} after_offset={} before_auto_scroll={} after_auto_scroll={} before_tail_offset={} after_tail_offset={} source_total_lines={} pending_load_older={}",
        action,
        key.code,
        modifiers,
        total_lines,
        view.last_visible_height,
        window_changed,
        before_limit,
        view.render_line_limit(),
        before_offset,
        view.scroll_offset,
        before_auto_scroll,
        view.auto_scroll,
        before_tail_offset,
        view.history_window_tail_offset,
        view.source_total_lines,
        view.pending_load_older
    );
    if window_changed {
        app.mark_output_dirty();
        crate::tui::log_debug!(
            "tui.output.scroll_dirty source=keyboard action={} reason=history_window_changed dirty_output=true",
            action
        );
    }
    true
}
