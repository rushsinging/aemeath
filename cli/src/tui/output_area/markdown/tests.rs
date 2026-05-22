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
    assert_eq!(spans[1].style.fg, Some(theme::CODE));
    assert_eq!(spans[1].style.bg, None);
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

#[test]
fn inline_code_wrap_preserves_code_style_across_boundary() {
    let lines = inline_markdown_lines("prefix `RedisSessionStore` suffix", base(), 16);

    assert_eq!(lines.len(), 2);
    assert_eq!(line_text(&lines[0]), "prefix RedisSess");
    assert_eq!(line_text(&lines[1]), "ionStore suffix");
    assert_eq!(code_cells(&lines[0]), "RedisSess");
    assert_eq!(code_cells(&lines[1]), "ionStore");
}

#[test]
fn inline_markdown_wrap_handles_marker_on_boundary() {
    let lines = inline_markdown_lines("123456789`RedisSessionSto`", base(), 10);

    assert_eq!(line_text(&lines[0]), "123456789R");
    assert_eq!(line_text(&lines[1]), "edisSessio");
    assert_eq!(line_text(&lines[2]), "nSto");
    assert_eq!(code_cells(&lines[0]), "R");
    assert_eq!(code_cells(&lines[1]), "edisSessio");
    assert_eq!(code_cells(&lines[2]), "nSto");
}

#[test]
fn inline_markdown_wrap_empty_string_keeps_one_line() {
    let lines = inline_markdown_lines("", base(), 10);

    assert_eq!(lines.len(), 1);
    assert_eq!(line_text(&lines[0]), "");
}

fn line_text(line: &ratatui::text::Line<'static>) -> String {
    line.spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect()
}

fn code_cells(line: &ratatui::text::Line<'static>) -> String {
    line.spans
        .iter()
        .filter(|span| span.style.fg == Some(theme::CODE))
        .map(|span| span.content.as_ref())
        .collect()
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
    let base = Style::default().fg(theme::SUCCESS);
    let lines = &["| Name | Value |", "| --- | --- |", "| foo  | bar   |"];
    let rendered = render_table_block(lines, base);
    assert_eq!(rendered.len(), 3);
    // header should be bold
    let header_spans = &rendered[0];
    assert!(header_spans.iter().any(|s| s.content.contains("Name")));
}
