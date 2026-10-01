use super::*;

#[test]
fn wrap_input_lines_tracks_original_row_and_col() {
    let lines = wrap_input_lines_for_width(vec!["abcdef"], 4);

    assert_eq!(
        lines,
        vec![
            WrappedInputLine {
                original_row: 0,
                original_col_start: 0,
                text: "abcd".to_string(),
            },
            WrappedInputLine {
                original_row: 0,
                original_col_start: 4,
                text: "ef".to_string(),
            },
        ]
    );
}

#[test]
fn display_position_for_anchor_maps_wrapped_col() {
    let lines = wrap_input_lines_for_width(vec!["abcdef"], 4);

    assert_eq!(display_position_for_anchor(&lines, 0, 5), (1, 1));
}

#[test]
fn display_position_for_anchor_uses_textarea_char_column_for_cjk() {
    let lines = wrap_input_lines_for_width(vec!["你好ab"], 10);

    assert_eq!(display_position_for_anchor(&lines, 0, 1), (0, 1));
}

#[test]
fn anchor_for_display_position_maps_wrapped_row_back_to_original_anchor() {
    let lines = wrap_input_lines_for_width(vec!["abcdef"], 4);

    assert_eq!(anchor_for_display_position(&lines, 1, 1), (0, 5));
}

#[test]
fn wrap_input_lines_uses_display_width_for_cjk() {
    let lines = wrap_input_lines_for_width(vec!["你好ab"], 4);

    assert_eq!(lines[0].text, "你好");
    assert_eq!(lines[1].original_col_start, 2);
}
