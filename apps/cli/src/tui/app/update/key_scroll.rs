use crate::tui::app::App;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

pub(super) fn handle_scroll_key(app: &mut App, key: KeyEvent, modifiers: KeyModifiers) -> bool {
    // 滚动真相归 view_state；widget 镜像由每帧 `refresh_output_scroll_from_view_state` 写回。
    // 总行数由 widget 的 document 提供（view_state 不持有 document）。
    let total_lines = app.output_area.document().total_lines();
    let view = &mut app.view_state.output;
    match (modifiers, key.code) {
        (KeyModifiers::NONE, KeyCode::PageUp) => view.scroll_up(10, total_lines),
        (KeyModifiers::NONE, KeyCode::PageDown) => view.scroll_down(10),
        (KeyModifiers::SHIFT, KeyCode::Up) => view.scroll_up(1, total_lines),
        (KeyModifiers::SHIFT, KeyCode::Down) => view.scroll_down(1),
        (KeyModifiers::SHIFT, KeyCode::Home) => {
            let was_expanded = view.expanded;
            view.scroll_to_top(total_lines);
            // 懒加载：首次 scroll_to_top 展开裁剪，需要重建 document
            if !was_expanded && view.expanded {
                app.mark_output_dirty();
            }
        }
        (KeyModifiers::SHIFT, KeyCode::End) => view.scroll_to_bottom(),
        _ => return false,
    }

    true
}
