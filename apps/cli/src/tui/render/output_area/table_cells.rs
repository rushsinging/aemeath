//! 表格单元格坐标：消费渲染期写入的单元格区间，做吸附与高亮折算。
//!
//! 列区间由渲染期（`markdown::table::render_table_block_with_rows`）在拼装 spans 时
//! 一并产出，挂在 `RenderedLine.table_row` 上；这里只做「屏幕字符偏移 → 列序号」与
//! 「列范围 → 字符区间」的纯换算，**不再解析 `│` / padding**（单一真相源）。
//!
//! blockquote 前缀也是 `│ `，因此本模块只处理已标记的表格数据行；
//! 「这行是不是表格数据行」由 `RenderedLine.table_row` 判定，不在这里猜。

use crate::tui::render::output::rendered::CellSpan;
use crate::tui::view_state::output::CellAnchor;

/// 字符偏移 → 单元格列序号。
///
/// 命中格内取本格；落在分隔符/格间 padding 上取右侧列；越过最后一格归最后一列。
pub(crate) fn cell_col_at(cells: &[CellSpan], char_idx: usize) -> Option<usize> {
    if cells.is_empty() {
        return None;
    }
    for (col, span) in cells.iter().enumerate() {
        if char_idx <= span.end {
            return Some(col);
        }
    }
    Some(cells.len() - 1)
}

/// 单元格列范围 → 该物理行需要高亮的字符区间 `[start, end)`。
///
/// 覆盖范围含中间分隔符，便于整段高亮；返回 `None` 表示该行没有对应列。
pub(crate) fn cell_range_char_span(
    cells: &[CellSpan],
    first_col: usize,
    last_col: usize,
) -> Option<(usize, usize)> {
    let selected: Vec<&CellSpan> = cells
        .iter()
        .enumerate()
        .filter(|(col, _)| *col >= first_col && *col <= last_col)
        .map(|(_, span)| span)
        .collect();
    let first = selected.first()?;
    let last = selected.last()?;
    Some((first.start.min(last.start), last.end))
}

impl super::OutputArea {
    /// 屏幕坐标 → 表格单元格锚点；落在非表格数据行（含 blockquote、分隔行）返回 `None`。
    ///
    /// 是否为表格数据行、各列位置在哪，全部来自渲染期元数据，避免对 `│` 做猜测。
    pub fn table_cell_at(
        &self,
        row: u16,
        col: u16,
        rect: &ratatui::layout::Rect,
        live_status: &crate::tui::view_model::LiveStatusViewModel,
    ) -> Option<CellAnchor> {
        let (logic_idx, char_idx) = self.screen_to_anchor(row, col, rect, live_status)?;
        let table_row = self.document.line_at(logic_idx)?.table_row.as_ref()?;
        let cell_col = cell_col_at(&table_row.cells, char_idx.as_usize())?;
        Some(CellAnchor {
            line: logic_idx,
            col: cell_col,
        })
    }

    /// 单元格选区命中的表格行：`(块序号, 表格实例序号, 数据行号, 逻辑行号)`，按文档顺序返回。
    ///
    /// 选中范围先映射成 `(块, 表格, 数据行)` 集合，再把同一数据行折行产生的所有物理行
    /// 一并纳入——折行续行可能落在拖拽行范围之外，但语义上属于同一格。
    /// 表格实例序号不可省：同一消息里多个表格的数据行号都从 0 起。
    pub(crate) fn selected_cell_lines(
        &self,
        start_line: usize,
        end_line: usize,
    ) -> Vec<(usize, usize, usize, usize)> {
        let mut keys: Vec<(usize, usize, usize)> = Vec::new();
        let mut logic_idx = 0usize;
        for (block_index, block) in self.document.blocks.iter().enumerate() {
            for line in block.lines.iter() {
                let in_range = logic_idx >= start_line && logic_idx <= end_line;
                logic_idx += 1;
                if !in_range {
                    continue;
                }
                let Some(table_row) = line.table_row.as_ref() else {
                    continue;
                };
                let key = (block_index, table_row.table, table_row.data_row);
                if !keys.contains(&key) {
                    keys.push(key);
                }
            }
        }
        if keys.is_empty() {
            return Vec::new();
        }

        let mut lines = Vec::new();
        let mut logic_idx = 0usize;
        for (block_index, block) in self.document.blocks.iter().enumerate() {
            for line in block.lines.iter() {
                if let Some(table_row) = line.table_row.as_ref() {
                    if keys.contains(&(block_index, table_row.table, table_row.data_row)) {
                        lines.push((block_index, table_row.table, table_row.data_row, logic_idx));
                    }
                }
                logic_idx += 1;
            }
        }
        lines
    }

    /// 单元格选区在该逻辑行需要高亮的字符区间 `[start, end)`。
    ///
    /// `selected_lines` 由 `selected_cell_lines` 预先算出（含折行续行），
    /// 分隔行与表格外的行不在其中，不参与高亮。
    pub(crate) fn cell_selection_span_for_line(
        &self,
        view: &crate::tui::view_state::output::OutputViewState,
        logic_idx: usize,
        selected_lines: &[usize],
    ) -> Option<(usize, usize)> {
        let (start, end) = view.cell_selection_range()?;
        if !selected_lines.contains(&logic_idx) {
            return None;
        }
        let table_row = self.document.line_at(logic_idx)?.table_row.as_ref()?;
        cell_range_char_span(&table_row.cells, start.col, end.col)
    }
}

#[cfg(test)]
#[path = "table_cells_tests.rs"]
mod tests;
