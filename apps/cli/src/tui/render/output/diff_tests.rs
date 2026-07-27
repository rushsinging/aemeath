use super::*;

fn line_text(spans: &[SpanPart]) -> String {
    spans.iter().map(|span| span.text.as_str()).collect()
}

#[test]
fn test_line_num_width() {
    assert_eq!(line_num_width(0), 1);
    assert_eq!(line_num_width(1), 1);
    assert_eq!(line_num_width(9), 1);
    assert_eq!(line_num_width(10), 2);
    assert_eq!(line_num_width(99), 2);
    assert_eq!(line_num_width(100), 3);
    assert_eq!(line_num_width(1000), 4);
}

#[test]
fn test_build_diff_lines_basic() {
    let mut out = Vec::new();
    build_diff_lines(
        "line1\nline2\nline3\n",
        "line1\nchanged\nline3\n",
        None,
        &mut out,
    );
    assert_eq!(out.len(), 4);
    assert!(line_text(&out[1]).contains("- "));
    assert!(line_text(&out[1]).contains("line2"));
    assert!(line_text(&out[2]).contains("+ "));
    assert!(line_text(&out[2]).contains("changed"));
}

#[test]
fn test_build_diff_lines_with_line_numbers() {
    let mut out = Vec::new();
    build_diff_lines("a\nb\nc\n", "a\nx\nc\n", None, &mut out);
    assert!(out.iter().all(|spans| !spans.is_empty()));
    assert!(line_text(&out[1]).contains('2'));
    assert!(line_text(&out[2]).contains('2'));
}

#[test]
fn test_build_diff_lines_with_real_start_line_numbers() {
    let mut out = Vec::new();
    build_diff_lines_from("a\nb\nc\n", "a\nx\nc\n", 100, 100, None, &mut out);
    let delete = line_text(&out[1]);
    let insert = line_text(&out[2]);
    assert!(delete.starts_with("101"), "got: {delete:?}");
    assert!(insert.starts_with("     101"), "got: {insert:?}");
}

#[test]
fn test_build_diff_lines_with_syntax_highlight() {
    let mut out = Vec::new();
    build_diff_lines("fn old() {}\n", "fn new() {}\n", Some("rs"), &mut out);
    assert!(line_text(&out[1]).contains("+ "));
    assert!(out[1].len() > 2);
}

#[test]
fn test_build_diff_lines_highlights_insert_and_context_but_delete_is_plain_red() {
    let mut out = Vec::new();
    build_diff_lines(
        "fn same() {}\nfn old() {}\n",
        "fn same() {}\nfn new() {}\n",
        Some("rs"),
        &mut out,
    );
    let context = out
        .iter()
        .find(|spans| line_text(spans).contains("same"))
        .unwrap();
    let delete = out
        .iter()
        .find(|spans| line_text(spans).contains("old"))
        .unwrap();
    let insert = out
        .iter()
        .find(|spans| line_text(spans).contains("new"))
        .unwrap();
    assert!(context.len() > 3, "context 正文应走 syntect: {context:?}");
    assert!(insert.len() > 3, "insert 正文应走 syntect: {insert:?}");
    assert_eq!(
        delete.last().map(|span| (span.text.as_str(), span.color)),
        Some(("fn old() {}", DIFF_REMOVE_FG))
    );
}

#[test]
fn test_build_diff_lines_empty() {
    let mut out = Vec::new();
    build_diff_lines("", "", None, &mut out);
    assert!(out.is_empty());
}

#[test]
fn large_diff_records_all_output_and_highlighted_lines() {
    let old = (0..120)
        .map(|index| format!("fn item_{index}() {{ println!(\"旧 {index}\"); }}"))
        .collect::<Vec<_>>()
        .join("\n");
    let new = (0..120)
        .map(|index| {
            if index == 60 {
                format!("fn item_{index}() {{ println!(\"新 {index}\"); }}")
            } else {
                format!("fn item_{index}() {{ println!(\"旧 {index}\"); }}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    let mut out = Vec::new();

    let (_, snapshot) = crate::tui::render::performance::capture(|| {
        build_diff_lines(&old, &new, Some("rs"), &mut out);
    });

    assert_eq!(
        out.len(),
        121,
        "120 行中替换一行应产生 119 context + delete + insert"
    );
    assert_eq!(snapshot.diff_build_calls, 1);
    assert_eq!(snapshot.diff_build_output_lines, 121);
    assert_eq!(
        snapshot.syntax_highlighter_creations, 1,
        "单个 diff 的 context/insert 行必须复用同一个 HighlightLines"
    );
    assert_eq!(
        snapshot.syntax_highlight_calls, 120,
        "delete 行不高亮，其余全部高亮"
    );
    assert!(snapshot.diff_build_ns > 0);
}
