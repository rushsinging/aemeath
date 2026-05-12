use super::table::parse_table_cells;
use super::*;

fn base() -> Style {
    Style::default()
}

#[test]
fn plain_text() {
    let spans = inline_markdown_spans("hello world", base());
    assert_eq!(spans.len(), 1);
    assert_eq!(spans[0].content, "hello world");
}

#[test]
fn bold_text() {
    let spans = inline_markdown_spans("**bold**", base());
    assert_eq!(spans.len(), 1);
    assert_eq!(spans[0].content, "bold");
    assert!(spans[0].style.add_modifier.contains(Modifier::BOLD));
}

#[test]
fn italic_text() {
    let spans = inline_markdown_spans("*italic*", base());
    assert_eq!(spans.len(), 1);
    assert_eq!(spans[0].content, "italic");
}

#[test]
fn inline_code() {
    let spans = inline_markdown_spans("use `HashMap` here", base());
    assert_eq!(spans.len(), 3);
    assert_eq!(spans[0].content, "use ");
    assert_eq!(spans[1].content, "HashMap");
    assert_eq!(spans[2].content, " here");
}

#[test]
fn mixed_formatting() {
    let spans = inline_markdown_spans("**bold** and *italic*", base());
    assert_eq!(spans.len(), 3);
    assert_eq!(spans[0].content, "bold");
    assert_eq!(spans[1].content, " and ");
    assert_eq!(spans[2].content, "italic");
}

#[test]
fn strikethrough() {
    let spans = inline_markdown_spans("~~deleted~~", base());
    assert_eq!(spans.len(), 1);
    assert_eq!(spans[0].content, "deleted");
}

#[test]
fn unclosed_marker_outputs_literal() {
    let spans = inline_markdown_spans("**unclosed", base());
    assert_eq!(spans.len(), 1);
    assert_eq!(spans[0].content, "**unclosed");
}

#[test]
fn link() {
    let spans = inline_markdown_spans("click [here](https://example.com) now", base());
    assert_eq!(spans.len(), 3);
    assert_eq!(spans[0].content, "click ");
    assert_eq!(spans[1].content, "here");
    assert_eq!(spans[2].content, " now");
}

#[test]
fn link_with_cjk_text() {
    let spans = inline_markdown_spans("click [你好](https://example.com) now", base());
    assert_eq!(spans.len(), 3);
    assert_eq!(spans[0].content, "click ");
    assert_eq!(spans[1].content, "你好");
    assert_eq!(spans[2].content, " now");
}

#[test]
fn empty_string() {
    let spans = inline_markdown_spans("", base());
    assert_eq!(spans.len(), 0);
}

#[test]
fn multiple_bold_spans() {
    let spans = inline_markdown_spans("**a** and **b**", base());
    assert_eq!(spans.len(), 3);
    assert_eq!(spans[0].content, "a");
    assert_eq!(spans[1].content, " and ");
    assert_eq!(spans[2].content, "b");
}

// ── Table tests ──

#[test]
fn table_separator_detection() {
    assert!(is_table_separator("|---|---|"));
    assert!(is_table_separator("| --- | --- |"));
    assert!(is_table_separator("|:---|:---:|---:|"));
    assert!(is_table_separator("| --- | :---: | ---: |"));
    assert!(!is_table_separator("| hello | world |"));
    assert!(!is_table_separator("just text"));
}

#[test]
fn table_row_detection() {
    assert!(is_table_row("| hello | world |"));
    assert!(is_table_row("| a | b | c |"));
    assert!(!is_table_row("just text"));
    assert!(!is_table_row("|---|---|"));
}

#[test]
fn test_parse_table_cells() {
    assert_eq!(
        parse_table_cells("| hello | world |"),
        vec!["hello", "world"]
    );
    assert_eq!(parse_table_cells("| a | b | c |"), vec!["a", "b", "c"]);
    assert_eq!(parse_table_cells("| single |"), vec!["single"]);
}

#[test]
fn test_render_simple_table() {
    let base = Style::default().fg(Color::Green);
    let lines = &["| Name | Value |", "| --- | --- |", "| foo  | bar   |"];
    let rendered = render_table_block(lines, base);
    assert_eq!(rendered.len(), 3);
    // header should be bold
    let header_spans = &rendered[0];
    assert!(header_spans.iter().any(|s| s.content.contains("Name")));
}
