use super::*;

fn span(start: usize, end: usize) -> CellSpan {
    CellSpan { start, end }
}

/// 与渲染期同构的行：` a │ b `（首 padding 1，分隔 ` │ `，尾 padding 1）。
fn row_cells() -> Vec<CellSpan> {
    vec![span(1, 2), span(5, 6)]
}

#[test]
fn cell_col_at_maps_char_offset_to_column() {
    let cells = row_cells();
    // 格内。
    assert_eq!(cell_col_at(&cells, 1), Some(0));
    // 格尾与分隔符之间的空白归右侧列。
    assert_eq!(cell_col_at(&cells, 3), Some(1));
    // 分隔符本身归右侧列。
    assert_eq!(cell_col_at(&cells, 4), Some(1));
    // 行尾 padding 归最后一列。
    assert_eq!(cell_col_at(&cells, 8), Some(1));
}

#[test]
fn cell_col_at_returns_none_without_cells() {
    assert_eq!(cell_col_at(&[], 0), None);
}

#[test]
fn cell_range_char_span_covers_selected_columns_including_gap() {
    let cells = vec![span(1, 2), span(5, 6), span(9, 10)];
    assert_eq!(cell_range_char_span(&cells, 0, 1), Some((1, 6)));
    assert_eq!(cell_range_char_span(&cells, 1, 2), Some((5, 10)));
}

#[test]
fn cell_range_char_span_handles_empty_cell_fragment() {
    // 折行续行里该列已无内容：区间退化在起点，仍参与高亮范围计算。
    let cells = vec![span(1, 7), span(10, 10)];
    assert_eq!(cell_range_char_span(&cells, 1, 1), Some((10, 10)));
}

#[test]
fn cell_range_char_span_returns_none_for_columns_out_of_range() {
    let cells = row_cells();
    assert_eq!(cell_range_char_span(&cells, 5, 6), None);
}
