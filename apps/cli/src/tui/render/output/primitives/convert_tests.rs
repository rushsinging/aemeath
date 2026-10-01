use super::*;
use ratatui::style::Color;

#[test]
fn test_spanparts_to_spans_preserves_text_and_color() {
    let parts = vec![
        SpanPart::plain("ab", Color::Red),
        SpanPart::plain("c", Color::Blue),
    ];
    let spans = spanparts_to_spans(&parts);

    assert_eq!(spans.len(), 2);
    assert_eq!(spans[0].content.as_ref(), "ab");
    assert_eq!(spans[0].style.fg, Some(Color::Red));
}

#[test]
fn test_rendered_line_from_spanparts_sets_plain() {
    let parts = vec![
        SpanPart::plain("  - ", Color::Red),
        SpanPart::plain("x", Color::Red),
    ];
    let line = rendered_line_from_spanparts(&parts);

    assert_eq!(line.plain, "  - x");
}
