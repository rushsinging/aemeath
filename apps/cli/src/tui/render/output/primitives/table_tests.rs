use super::*;
use ratatui::style::Style;

#[test]
fn test_table_renders_rows_with_aligned_plain() {
    let src = ["| a | bb |", "|---|----|", "| 1 | 2 |"];
    let lines = table(&src, Style::default(), 40);

    assert!(!lines.is_empty());
    assert!(lines
        .iter()
        .any(|line| line.plain.contains('│') || line.plain.contains('|')));
}
