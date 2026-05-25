use super::content::format_ask_user_option_lines;
use super::{LineStyle, OutputArea};

fn is_ask_user(style: LineStyle) -> bool {
    matches!(style, LineStyle::AskUser)
}

fn is_normal(style: LineStyle) -> bool {
    matches!(style, LineStyle::Normal)
}

#[test]
fn test_format_ask_user_option_lines_splits_newline_options() {
    let lines = format_ask_user_option_lines(0, "A\nB\nC", true, false);

    assert_eq!(lines, vec!["  ❯ 1. A", "       B", "       C"]);
}

#[test]
fn test_format_ask_user_option_lines_preserves_single_line_option() {
    let lines = format_ask_user_option_lines(1, "Only one", false, false);

    assert_eq!(lines, vec!["    2. Only one"]);
}

#[test]
fn test_format_ask_user_option_lines_handles_empty_option() {
    let lines = format_ask_user_option_lines(2, "", false, true);

    assert_eq!(lines, vec!["  [ ] 3. "]);
}

#[test]
fn test_push_ask_user_renders_each_newline_option_on_own_line() {
    let mut output = OutputArea::new();
    let options = vec!["A\nB\nC".to_string()];

    let start = output
        .push_ask_user("Choose", &options, Some("A\nB\nC"), false)
        .expect("options should have a start index");

    assert_eq!(output.lines[start].content, "  ❯ 1. A");
    assert!(is_ask_user(output.lines[start].style));
    assert_eq!(output.lines[start + 1].content, "       B");
    assert!(is_normal(output.lines[start + 1].style));
    assert_eq!(output.lines[start + 2].content, "       C");
}

#[test]
fn test_update_ask_user_options_updates_multiline_option_range() {
    let mut output = OutputArea::new();
    let options = vec!["A\nB".to_string(), "C".to_string()];
    let start = output
        .push_ask_user("Choose", &options, Some("A\nB"), false)
        .expect("options should have a start index");
    let ranges = vec![start..start + 2, start + 2..start + 3];

    output.update_ask_user_options(&ranges, &options, 1, false, &[false, false]);

    assert_eq!(output.lines[start].content, "    1. A");
    assert_eq!(output.lines[start + 1].content, "       B");
    assert_eq!(output.lines[start + 2].content, "  ❯ 2. C");
    assert!(is_ask_user(output.lines[start + 2].style));
}
