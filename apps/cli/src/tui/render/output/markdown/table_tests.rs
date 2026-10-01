use super::*;

#[test]
fn test_is_table_separator() {
    assert!(is_table_separator("|---|---|"));
    assert!(is_table_separator("| :---: | ---: |"));
    assert!(!is_table_separator("| hello | world |"));
}

#[test]
fn test_is_table_row() {
    assert!(is_table_row("| hello | world |"));
    assert!(!is_table_row("|---|---|"));
}

#[test]
fn test_render_table_block_basic() {
    let lines = vec!["| a | b |", "|---|---|", "| 1 | 2 |"];
    let result = render_table_block(&lines, Style::default(), 80);
    // header + separator + data = 3 行
    assert_eq!(result.len(), 3);
}

#[test]
fn test_render_table_block_wrap() {
    // 一个很窄的宽度，应该触发换行
    let lines = vec!["| hello world | foo |", "|---|---|", "| 1 | 2 |"];
    let result = render_table_block(&lines, Style::default(), 20);
    // header 行可能被换行成多行
    assert!(result.len() >= 3, "should have at least 3 rows");
}

#[test]
fn test_constrain_column_widths_no_constraint() {
    let natural = vec![5, 10, 3];
    let result = constrain_column_widths(&natural, 3, 100);
    assert_eq!(result, vec![5, 10, 3]);
}

#[test]
fn test_constrain_column_widths_constrained() {
    let natural = vec![20, 30, 40];
    let result = constrain_column_widths(&natural, 3, 40);
    let total: usize = result.iter().sum();
    assert!(total <= 40 - 8, "total {total} should fit in budget");
}
