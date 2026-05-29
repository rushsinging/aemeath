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
mod tests {
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
}
