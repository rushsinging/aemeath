//! table 原语：复用现有 render_table_block(产出 Vec<Vec<Span>>) 转 RenderedLine。

use crate::tui::render::output::markdown::render_table_block;
use crate::tui::render::output::rendered::RenderedLine;
use ratatui::style::Style;

pub fn table(src_lines: &[&str], base_style: Style, width: u16) -> Vec<RenderedLine> {
    render_table_block(src_lines, base_style, width as usize)
        .into_iter()
        .map(RenderedLine::new)
        .collect()
}

#[cfg(test)]
#[path = "table_tests.rs"]
mod tests;
