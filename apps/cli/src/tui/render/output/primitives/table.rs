//! table 原语：复用现有 render_table_block(产出 Vec<Vec<Span>>) 转 RenderedLine。

use crate::tui::render::output::markdown::render_table_block_with_rows;
use crate::tui::render::output::rendered::RenderedLine;
use ratatui::style::Style;

pub fn table(src_lines: &[&str], base_style: Style, width: u16) -> Vec<RenderedLine> {
    let (rows, row_refs) = render_table_block_with_rows(src_lines, base_style, width as usize);
    rows.into_iter()
        .zip(row_refs)
        .map(|(spans, table_row)| match table_row {
            // 折行产生的多个物理行共享同一数据行序号，供单元格选区/复制拼回同一格。
            Some(row) => RenderedLine::new(spans).with_table_row_ref(row),
            None => RenderedLine::new(spans),
        })
        .collect()
}

#[cfg(test)]
#[path = "table_tests.rs"]
mod tests;
