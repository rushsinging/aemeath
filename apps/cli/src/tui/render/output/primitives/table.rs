//! table 原语：复用现有 render_table_block(产出 Vec<Vec<Span>>) 转 RenderedLine。

use crate::tui::render::output::markdown::render_table_block_with_rows;
use crate::tui::render::output::rendered::{RenderedLine, TableRowRef};
use ratatui::style::Style;

/// 渲染一个表格块。
///
/// `table_index` 是该表格在所属消息块内的实例序号：同一消息可有多个表格，
/// 其数据行号都从 0 开始，选区/复制分组必须带上它才能区分表格实例。
pub fn table(
    src_lines: &[&str],
    base_style: Style,
    width: u16,
    table_index: usize,
) -> Vec<RenderedLine> {
    let (rows, row_refs) = render_table_block_with_rows(src_lines, base_style, width as usize);
    rows.into_iter()
        .zip(row_refs)
        .map(|(spans, table_row)| match table_row {
            // 折行产生的多个物理行共享同一数据行序号，供单元格选区/复制拼回同一格。
            Some(row) => RenderedLine::new(spans).with_table_row_ref(TableRowRef {
                table: table_index,
                data_row: row.data_row,
                cells: row.cells,
            }),
            None => RenderedLine::new(spans),
        })
        .collect()
}

#[cfg(test)]
#[path = "table_tests.rs"]
mod tests;
