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
    let result = render_table_block_with_rows(&lines, Style::default(), 80).0;
    // header + separator + data = 3 行
    assert_eq!(result.len(), 3);
}

#[test]
fn test_render_table_block_wrap() {
    // 一个很窄的宽度，应该触发换行
    let lines = vec!["| hello world | foo |", "|---|---|", "| 1 | 2 |"];
    let result = render_table_block_with_rows(&lines, Style::default(), 20).0;
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

fn line_plain(spans: &[Span<'static>]) -> String {
    spans.iter().map(|s| s.content.as_ref()).collect()
}

fn slice_cell(plain: &str, cell: &crate::tui::render::output::rendered::CellSpan) -> String {
    plain
        .chars()
        .skip(cell.start)
        .take(cell.end.saturating_sub(cell.start))
        .collect()
}

#[test]
fn render_table_block_with_rows_reports_exact_cell_spans() {
    let lines = vec!["| A | B |", "| --- | --- |", "| 1 | 22 |"];
    let (rows, refs) = render_table_block_with_rows(&lines, Style::default(), 80);
    assert_eq!(rows.len(), refs.len(), "行归属必须与物理行一一对应");
    assert_eq!(refs[1], None, "分隔行不携带数据行");

    let header = refs[0].as_ref().expect("表头是数据行 0");
    assert_eq!(header.data_row, 0);
    let header_plain = line_plain(&rows[0]);
    assert_eq!(slice_cell(&header_plain, &header.cells[0]), "A");
    assert_eq!(slice_cell(&header_plain, &header.cells[1]), "B");

    let data = refs[2].as_ref().expect("数据行 1");
    assert_eq!(data.data_row, 1);
    let data_plain = line_plain(&rows[2]);
    assert_eq!(slice_cell(&data_plain, &data.cells[0]), "1");
    assert_eq!(slice_cell(&data_plain, &data.cells[1]), "22");
    // 单元格区间不含 padding：起点前/终点后是 padding 或分隔符。
    assert_eq!(data_plain.chars().nth(data.cells[1].end), Some(' '));
}

#[test]
fn render_table_block_with_rows_keeps_wrapped_rows_in_same_data_row() {
    let lines = vec!["| ABCDEFGHIJKLMNOP | b |", "| --- | --- |", "| x | y |"];
    let (rows, refs) = render_table_block_with_rows(&lines, Style::default(), 20);
    let header_rows: Vec<usize> = refs
        .iter()
        .filter_map(|row| row.as_ref().map(|row| row.data_row))
        .filter(|row| *row == 0)
        .collect();
    assert!(header_rows.len() >= 2, "首列应按列宽折成多条物理行");

    // 折行物理行的片段按列拼接后等于原单元格内容。
    let mut first_col = String::new();
    for (row, row_ref) in rows.iter().zip(refs.iter()) {
        let Some(row_ref) = row_ref.as_ref() else {
            continue;
        };
        if row_ref.data_row != 0 {
            continue;
        }
        first_col.push_str(&slice_cell(&line_plain(row), &row_ref.cells[0]));
    }
    assert_eq!(first_col, "ABCDEFGHIJKLMNOP");
}
