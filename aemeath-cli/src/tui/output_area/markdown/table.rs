use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;

/// 检测 Markdown 表格分隔行，如 `|---|---|`、`| :---: | ---: |`
pub fn is_table_separator(line: &str) -> bool {
    let trimmed = line.trim();
    if !trimmed.starts_with('|') || !trimmed.ends_with('|') || trimmed.len() <= 2 {
        return false;
    }
    let inner = trimmed.get(1..trimmed.len() - 1).unwrap_or("").trim();
    inner.split('|').all(|seg| {
        let seg = seg.trim();
        seg.starts_with(':') || seg.starts_with('-') || seg.is_empty()
    }) && inner.split('|').any(|seg| seg.trim().contains('-'))
}

/// 检测 Markdown 表格数据行，如 `| hello | world |`
pub fn is_table_row(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.starts_with('|')
        && trimmed.ends_with('|')
        && trimmed.len() > 2
        && !is_table_separator(line)
}

/// 解析表格行中的单元格内容
pub fn parse_table_cells(line: &str) -> Vec<&str> {
    let trimmed = line.trim();
    if trimmed.len() <= 2 {
        return vec![];
    }
    let trimmed = trimmed.get(1..trimmed.len() - 1).unwrap_or("");
    trimmed.split('|').map(|s| s.trim()).collect()
}

/// 渲染整个表格块为 Vec<Vec<Span>>（每行一组 spans）。
pub fn render_table_block(lines: &[&str], base_style: Style) -> Vec<Vec<Span<'static>>> {
    if lines.is_empty() {
        return vec![];
    }

    let mut all_cells: Vec<Vec<String>> = Vec::new();
    let mut separator_idx: Option<usize> = None;
    for (i, line) in lines.iter().enumerate() {
        if is_table_separator(line) {
            separator_idx = Some(i);
        } else {
            all_cells.push(
                parse_table_cells(line)
                    .into_iter()
                    .map(|s| s.to_string())
                    .collect(),
            );
        }
    }

    let num_cols = all_cells.iter().map(|r| r.len()).max().unwrap_or(0);
    if num_cols == 0 {
        return lines
            .iter()
            .map(|l| vec![Span::styled(l.to_string(), base_style)])
            .collect();
    }

    let col_widths = column_widths(&all_cells, num_cols);
    let mut result = Vec::new();
    let mut data_row_idx = 0;
    let header_style = base_style.add_modifier(Modifier::BOLD);
    let border_style = base_style.fg(Color::DarkGray);

    for (i, line) in lines.iter().enumerate() {
        if is_table_separator(line) {
            result.push(separator_spans(&col_widths, border_style));
        } else {
            let cells = if data_row_idx < all_cells.len() {
                &all_cells[data_row_idx]
            } else {
                continue;
            };
            data_row_idx += 1;
            let is_header = separator_idx.map_or(true, |si| i < si);
            let style = if is_header { header_style } else { base_style };
            result.push(row_spans(cells, &col_widths, style, border_style));
        }
    }

    result
}

fn column_widths(all_cells: &[Vec<String>], num_cols: usize) -> Vec<usize> {
    let mut col_widths = vec![0usize; num_cols];
    for row in all_cells {
        for (c, cell) in row.iter().enumerate() {
            if c < num_cols {
                use unicode_width::UnicodeWidthStr;
                col_widths[c] = col_widths[c].max(cell.width());
            }
        }
    }
    col_widths
}

fn separator_spans(col_widths: &[usize], border_style: Style) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    for (c, &w) in col_widths.iter().enumerate() {
        if c > 0 {
            spans.push(Span::styled("┼".to_string(), border_style));
        }
        spans.push(Span::styled("─".repeat(w + 2), border_style));
    }
    spans
}

fn row_spans(
    cells: &[String],
    col_widths: &[usize],
    style: Style,
    border_style: Style,
) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let num_cols = col_widths.len();
    for (c, &w) in col_widths.iter().enumerate() {
        if c > 0 {
            spans.push(Span::styled(" │ ".to_string(), border_style));
        } else {
            spans.push(Span::styled(" ".to_string(), style));
        }
        let cell = cells.get(c).map(|s| s.as_str()).unwrap_or("");
        use unicode_width::UnicodeWidthStr;
        let cell_width = cell.width();
        let padded = if cell_width < w {
            format!("{}{}", cell, " ".repeat(w - cell_width))
        } else {
            cell.to_string()
        };
        spans.push(Span::styled(padded, style));

        if c == num_cols - 1 {
            spans.push(Span::styled(" ".to_string(), style));
        }
    }
    spans
}
