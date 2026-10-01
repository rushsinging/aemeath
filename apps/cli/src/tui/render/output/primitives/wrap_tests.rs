use super::*;
use ratatui::style::{Color, Style};

#[test]
fn test_wrap_spans_to_rendered_lines_splits_ascii_by_width() {
    let lines = wrap_spans_to_rendered_lines(vec![Span::raw("abcdef")], 4);

    assert_eq!(
        lines
            .iter()
            .map(|line| line.plain.as_str())
            .collect::<Vec<_>>(),
        vec!["abcd", "ef"]
    );
}

#[test]
fn test_wrap_spans_to_rendered_lines_preserves_style_across_wrap() {
    let style = Style::default().fg(Color::Red);
    let lines = wrap_spans_to_rendered_lines(vec![Span::styled("abcdef", style)], 4);

    assert_eq!(lines[0].spans[0].style.fg, Some(Color::Red));
    assert_eq!(lines[1].spans[0].style.fg, Some(Color::Red));
}

#[test]
fn test_wrap_spans_to_rendered_lines_handles_cjk_display_width() {
    let lines = wrap_spans_to_rendered_lines(vec![Span::raw("你好ab")], 4);

    assert_eq!(
        lines
            .iter()
            .map(|line| line.plain.as_str())
            .collect::<Vec<_>>(),
        vec!["你好", "ab"]
    );
}

#[test]
fn test_wrap_spans_with_prefix_indents_continuation_lines() {
    let lines = wrap_spans_with_prefix(
        vec![Span::raw("> abcdef")],
        6,
        Some(Span::raw("  ")),
        WrapMode::Char,
    );

    assert_eq!(
        lines
            .iter()
            .map(|line| line.plain.as_str())
            .collect::<Vec<_>>(),
        vec!["> abcd", "  ef"]
    );
}

#[test]
fn test_wrap_text_to_strings_word_mode_breaks_at_word_boundary() {
    // Word 模式：优先在空格处断行，不拆词
    let lines = wrap_text_to_strings("aaa bbb ccc ddd", 7, WrapMode::Word);
    assert_eq!(lines, vec!["aaa bbb", "ccc ddd"], "Word 模式应在词边界断行");
}

#[test]
fn test_wrap_text_to_strings_word_mode_falls_back_to_char_for_overlong_word() {
    // 单个超长词仍需字符断，否则溢出
    let lines = wrap_text_to_strings("aaaaaaaaaa", 4, WrapMode::Word);
    assert_eq!(lines, vec!["aaaa", "aaaa", "aa"], "超长词应字符回退断行");
}

#[test]
fn test_wrap_text_to_strings_word_mode_cjk_equivalent() {
    // CJK 无词边界，Word 与 Char 等价（逐字符按宽度断）
    let lines = wrap_text_to_strings("你好世界你好", 4, WrapMode::Word);
    assert_eq!(lines, vec!["你好", "世界", "你好"]);
}

#[test]
fn test_wrap_text_to_strings_char_mode_keeps_current_behavior() {
    // Char 模式：逐字符硬切，保持现状
    let lines = wrap_text_to_strings("aaa bbb", 4, WrapMode::Char);
    assert_eq!(lines, vec!["aaa ", "bbb"]);
}

#[test]
fn test_wrap_entry_normalizes_tab_before_width_calc() {
    // issue #1670：tab 必须在宽度计算前展开，否则以 0 宽参与断行导致溢出/吞字。
    let lines = wrap_spans_with_prefix(vec![Span::raw("a\tb")], 80, None, WrapMode::Word);
    assert_eq!(lines[0].plain, "a    b");
}
