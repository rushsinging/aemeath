use crate::tui::core::App;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

pub(super) fn handle_scroll_key(
    app: &mut App,
    key: KeyEvent,
    modifiers: KeyModifiers,
) -> bool {
    match (modifiers, key.code) {
        (KeyModifiers::NONE, KeyCode::PageUp) => app.output_area.scroll_up(10),
        (KeyModifiers::NONE, KeyCode::PageDown) => app.output_area.scroll_down(10),
        (KeyModifiers::SHIFT, KeyCode::Up) => app.output_area.scroll_up(1),
        (KeyModifiers::SHIFT, KeyCode::Down) => app.output_area.scroll_down(1),
        (KeyModifiers::SHIFT, KeyCode::Home) => {
            app.output_area.scroll_up(app.output_area.line_count())
        }
        (KeyModifiers::SHIFT, KeyCode::End) => app.output_area.scroll_to_bottom(),
        _ => return false,
    }

    true
}
