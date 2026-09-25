use super::normalize_display_control_chars;

#[test]
fn test_tab_expands_to_four_spaces() {
    assert_eq!(normalize_display_control_chars("a\tb"), "a    b");
}

#[test]
fn test_multiple_tabs_expand_independently() {
    assert_eq!(
        normalize_display_control_chars("wanaka_session\t9892\t78"),
        "wanaka_session    9892    78"
    );
}

#[test]
fn test_newline_preserved() {
    assert_eq!(normalize_display_control_chars("a\nb"), "a\nb");
}

#[test]
fn test_carriage_return_replaced_and_newline_kept_in_crlf() {
    assert_eq!(normalize_display_control_chars("a\r\nb"), "a\u{fffd}\nb");
}

#[test]
fn test_escape_char_replaced_with_replacement_char() {
    assert_eq!(
        normalize_display_control_chars("a\u{1b}[31m"),
        "a\u{fffd}[31m"
    );
}

#[test]
fn test_c1_csi_replaced() {
    assert_eq!(normalize_display_control_chars("a\u{9b}0m"), "a\u{fffd}0m");
}

#[test]
fn test_bell_and_del_replaced() {
    assert_eq!(
        normalize_display_control_chars("a\u{7}b\u{7f}"),
        "a\u{fffd}b\u{fffd}"
    );
}

#[test]
fn test_plain_text_unchanged() {
    assert_eq!(
        normalize_display_control_chars("你好 hello 2026"),
        "你好 hello 2026"
    );
}

#[test]
fn test_zero_width_format_chars_preserved() {
    assert_eq!(normalize_display_control_chars("a\u{200b}b"), "a\u{200b}b");
}
