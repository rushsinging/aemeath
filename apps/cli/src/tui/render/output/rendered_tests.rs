use super::*;
use ratatui::style::{Color, Style};
use ratatui::text::Span;

#[test]
fn test_rendered_line_new_derives_plain_from_spans() {
    let line = RenderedLine::new(vec![
        Span::styled("Hello ", Style::default().fg(Color::Red)),
        Span::styled("世界", Style::default().fg(Color::Blue)),
    ]);

    assert_eq!(line.plain, "Hello 世界");
    assert_eq!(line.spans.len(), 2);
}

#[test]
fn test_rendered_line_with_plain_keeps_explicit_plain() {
    let line = RenderedLine::with_plain(vec![Span::raw("**x**")], "x".to_string());

    assert_eq!(line.plain, "x");
}

#[test]
fn test_rendered_line_with_fill_style_preserves_plain_text() {
    let fill = Style::default().bg(Color::Blue);
    let line = RenderedLine::from_plain("hello").with_fill_style(fill);

    assert_eq!(line.plain, "hello");
    assert_eq!(line.fill_style, Some(fill));
}

#[test]
fn test_rendered_line_empty_with_fill_style_has_no_filler_text() {
    let fill = Style::default().bg(Color::Blue);
    let line = RenderedLine::empty().with_fill_style(fill);

    assert_eq!(line.plain, "");
    assert!(line.spans.is_empty());
    assert_eq!(line.fill_style, Some(fill));
}

#[test]
fn test_rendered_document_total_lines_sums_blocks() {
    let doc = RenderedDocument::new(vec![
        RenderedBlock {
            block_id: "a".into(),
            lines: Rc::new(vec![RenderedLine::default(), RenderedLine::default()]),
        },
        RenderedBlock {
            block_id: "b".into(),
            lines: Rc::new(vec![RenderedLine::default()]),
        },
    ]);

    assert_eq!(doc.total_lines(), 3);
    assert_eq!(doc.iter_lines().count(), 3);
}

#[test]
fn line_at_crosses_empty_and_non_empty_blocks() {
    let doc = RenderedDocument::new(vec![
        RenderedBlock {
            block_id: "empty".into(),
            lines: Rc::new(Vec::new()),
        },
        RenderedBlock {
            block_id: "first".into(),
            lines: Rc::new(vec![
                RenderedLine::from_plain("zero"),
                RenderedLine::from_plain("one"),
            ]),
        },
        RenderedBlock {
            block_id: "second".into(),
            lines: Rc::new(vec![RenderedLine::from_plain("two")]),
        },
    ]);

    assert_eq!(doc.line_at(0).map(|line| line.plain.as_str()), Some("zero"));
    assert_eq!(doc.line_at(1).map(|line| line.plain.as_str()), Some("one"));
    assert_eq!(doc.line_at(2).map(|line| line.plain.as_str()), Some("two"));
    assert_eq!(doc.line_at(3), None);
}

#[test]
fn line_anchor_round_trips_across_prefixed_blocks() {
    let old = RenderedDocument::new(vec![
        RenderedBlock {
            block_id: "a".into(),
            lines: Rc::new(vec![RenderedLine::from_plain("a0")]),
        },
        RenderedBlock {
            block_id: "anchor".into(),
            lines: Rc::new(vec![
                RenderedLine::from_plain("anchor0"),
                RenderedLine::from_plain("anchor1"),
            ]),
        },
    ]);
    let expanded = RenderedDocument::new(vec![
        RenderedBlock {
            block_id: "earlier".into(),
            lines: Rc::new(vec![
                RenderedLine::from_plain("earlier0"),
                RenderedLine::from_plain("earlier1"),
            ]),
        },
        RenderedBlock {
            block_id: "a".into(),
            lines: Rc::new(vec![RenderedLine::from_plain("a0")]),
        },
        RenderedBlock {
            block_id: "anchor".into(),
            lines: Rc::new(vec![
                RenderedLine::from_plain("anchor0"),
                RenderedLine::from_plain("anchor1"),
            ]),
        },
    ]);

    let anchor = old.line_anchor_at(2).expect("old anchor");
    assert_eq!(expanded.line_index_for_anchor(&anchor), Some(4));
}

#[test]
fn lines_in_range_returns_global_indices_across_block_boundaries() {
    let doc = RenderedDocument::new(vec![
        RenderedBlock {
            block_id: "first".into(),
            lines: Rc::new(vec![
                RenderedLine::from_plain("zero"),
                RenderedLine::from_plain("one"),
            ]),
        },
        RenderedBlock {
            block_id: "empty".into(),
            lines: Rc::new(Vec::new()),
        },
        RenderedBlock {
            block_id: "second".into(),
            lines: Rc::new(vec![
                RenderedLine::from_plain("two"),
                RenderedLine::from_plain("three"),
            ]),
        },
    ]);

    let selected = doc
        .lines_in_range(1..3)
        .map(|(index, line)| (index, line.plain.as_str()))
        .collect::<Vec<_>>();

    assert_eq!(selected, vec![(1, "one"), (2, "two")]);
    assert_eq!(doc.lines_in_range(9..12).count(), 0);
}

#[test]
fn test_rendered_line_new_normalizes_control_chars() {
    let line = RenderedLine::new(vec![Span::raw("a\u{1b}b")]);
    assert_eq!(line.plain, "a\u{fffd}b");
    assert_eq!(line.spans[0].content.as_ref(), "a\u{fffd}b");
}

#[test]
fn test_with_plain_normalizes_spans_and_plain_symmetrically() {
    // 不变式：plain == spans 可见文本拼接 —— 两侧必须同函数归一化。
    let line = RenderedLine::with_plain(vec![Span::raw("a\tb")], "a\tb".to_string());
    assert_eq!(line.plain, "a    b");
    assert_eq!(line.spans[0].content.as_ref(), "a    b");
    assert_eq!(
        line.plain,
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>()
    );
}
