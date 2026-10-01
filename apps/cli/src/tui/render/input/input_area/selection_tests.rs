use super::*;
use crate::tui::view_state::InputSelectionViewState;

fn selection_view(start: (usize, usize), end: (usize, usize)) -> InputSelectionViewState {
    let mut view = InputSelectionViewState::default();
    view.begin_selection(start);
    view.update_selection(end);
    view
}

#[test]
fn test_selected_text_for_view_maps_cjk_screen_col_to_char_index() {
    let input = InputArea::new();
    let text = "你好a";
    let inner = Rect {
        x: 10,
        y: 5,
        width: 20,
        height: 3,
    };
    let start = input.screen_to_input_anchor(text, 5, 12, &inner);
    let end = input.screen_to_input_anchor(text, 5, 15, &inner);
    let view = selection_view(start, end);

    assert_eq!(
        input.selected_text_for_view(text, &view),
        Some("好a".to_string())
    );
}

#[test]
fn test_selected_text_for_view_maps_emoji_screen_col_to_char_index() {
    let input = InputArea::new();
    let text = "a🚀b";
    let inner = Rect {
        x: 10,
        y: 5,
        width: 20,
        height: 3,
    };
    let start = input.screen_to_input_anchor(text, 5, 11, &inner);
    let end = input.screen_to_input_anchor(text, 5, 14, &inner);
    let view = selection_view(start, end);

    assert_eq!(
        input.selected_text_for_view(text, &view),
        Some("🚀b".to_string())
    );
}

#[test]
fn test_screen_to_input_anchor_maps_screen_col_without_mutating_state() {
    let input = InputArea::new();
    let text = "你好a";
    let inner = Rect {
        x: 10,
        y: 5,
        width: 20,
        height: 3,
    };

    assert_eq!(input.screen_to_input_anchor(text, 5, 12, &inner), (0, 1));
    assert_eq!(input.screen_to_input_anchor(text, 5, 99, &inner), (0, 3));
    assert_eq!(input.screen_to_input_anchor(text, 8, 12, &inner), (3, 0));
}

#[test]
fn test_screen_to_input_anchor_maps_wrapped_display_row_to_original_anchor() {
    let input = InputArea::new();
    let text = "abcdef";
    let inner = Rect {
        x: 10,
        y: 5,
        width: 4,
        height: 3,
    };

    assert_eq!(input.screen_to_input_anchor(text, 6, 11, &inner), (0, 5));
}

#[test]
fn test_selected_text_for_view_boundary_end_col_clamps_to_line_len() {
    let input = InputArea::new();
    let text = "你好";
    let inner = Rect {
        x: 10,
        y: 5,
        width: 20,
        height: 3,
    };
    let start = input.screen_to_input_anchor(text, 5, 10, &inner);
    let end = input.screen_to_input_anchor(text, 5, 99, &inner);
    let view = selection_view(start, end);

    assert_eq!(
        input.selected_text_for_view(text, &view),
        Some("你好".to_string())
    );
}
