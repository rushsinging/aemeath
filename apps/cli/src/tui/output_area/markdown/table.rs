use ratatui::style::{Modifier, Style};
use ratatui::text::Span;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::tui::display::theme;

use super::inline_markdown_spans;

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
///
/// `available_width` 为终端可用宽度。当单元格内容总宽度超出时，自动按列宽换行。
/// 换行规则：每个单元格内容超过其列宽时，按列宽切分为多行，行尾不截断。
pub fn render_table_block(
    lines: &[&str],
    base_style: Style,
    available_width: usize,
) -> Vec<Vec<Span<'static>>> {
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

    // 计算列宽：取内容最大宽度，但受 available_width 限制
    let natural_widths = natural_column_widths(&all_cells, num_cols);
    let col_widths = constrain_column_widths(&natural_widths, num_cols, available_width);

    let mut result = Vec::new();
    let mut data_row_idx = 0;
    let header_style = base_style.add_modifier(Modifier::BOLD);
    let border_style = base_style.fg(theme::BORDER);

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
            let is_header = separator_idx.is_none_or(|si| i < si);
            let style = if is_header { header_style } else { base_style };
            let row = wrapped_row_spans(cells, &col_widths, style, border_style);
            result.extend(row);
        }
    }

    result
}

/// 按内容最大宽度计算自然列宽
fn natural_column_widths(all_cells: &[Vec<String>], num_cols: usize) -> Vec<usize> {
    let mut col_widths = vec![0usize; num_cols];
    for row in all_cells {
        for (c, cell) in row.iter().enumerate() {
            if c < num_cols {
                col_widths[c] = col_widths[c].max(cell.width());
            }
        }
    }
    col_widths
}

/// 将列宽约束到可用宽度内。
/// 优先保持自然列宽，超出时等比缩小。
fn constrain_column_widths(natural: &[usize], num_cols: usize, available: usize) -> Vec<usize> {
    // 每列开销：padding " " + 内容 + padding " "，列间 " │ " (3 chars)
    let overhead = 1 + num_cols.saturating_sub(1) * 3 + 1; // left pad + separators + right pad
    let content_budget = available.saturating_sub(overhead);

    let total_natural: usize = natural.iter().sum();
    if total_natural <= content_budget {
        return natural.to_vec();
    }

    // 等比缩小，每列至少 3 字符
    let min_col = 3;
    let mut result = vec![0usize; num_cols];
    let mut remaining_budget = content_budget;

    // 先分配最小宽度
    for (c, w) in result.iter_mut().enumerate() {
        *w = min_col.min(natural[c]);
        remaining_budget = remaining_budget.saturating_sub(*w);
    }

    // 按比例分配剩余空间给需要更多宽度的列
    let need_more: Vec<(usize, usize)> = natural
        .iter()
        .enumerate()
        .filter(|(c, n)| result[*c] < **n)
        .map(|(c, n)| (c, *n - result[c]))
        .collect();
    let total_need: usize = need_more.iter().map(|(_, d)| *d).sum();
    if total_need > 0 {
        for (c, deficit) in &need_more {
            let share = remaining_budget * *deficit / total_need;
            result[*c] += share;
        }
    }

    result
}

/// 将一个数据行的单元格按列宽换行，返回 1-N 行 spans。
/// 单元格内容会先解析 inline markdown（`code`、**bold** 等）再换行。
fn wrapped_row_spans(
    cells: &[String],
    col_widths: &[usize],
    style: Style,
    border_style: Style,
) -> Vec<Vec<Span<'static>>> {
    let num_cols = col_widths.len();
    // 每个单元格解析 inline markdown 得到 styled spans，再按列宽换行
    let wrapped_cells: Vec<Vec<Vec<Span<'static>>>> = (0..num_cols)
        .map(|c| {
            let cell = cells.get(c).map(|s| s.as_str()).unwrap_or("");
            let spans = inline_markdown_spans(cell, style);
            wrap_spans_to_width(spans, col_widths[c])
        })
        .collect();

    let max_lines = wrapped_cells.iter().map(|l| l.len()).max().unwrap_or(1);

    let mut rows = Vec::with_capacity(max_lines);
    for line_idx in 0..max_lines {
        let mut spans = Vec::new();
        for c in 0..num_cols {
            if c > 0 {
                spans.push(Span::styled(" │ ".to_string(), border_style));
            } else {
                spans.push(Span::styled(" ".to_string(), style));
            }
            let cell_spans = wrapped_cells[c]
                .get(line_idx)
                .cloned()
                .unwrap_or_else(|| vec![Span::styled(String::new(), style)]);

            // 计算已有宽度，补齐到列宽
            let cell_width: usize = cell_spans.iter().map(|s| s.content.width()).sum();
            let col_w = col_widths[c];
            if cell_width < col_w {
                spans.extend(cell_spans);
                spans.push(Span::styled(" ".repeat(col_w - cell_width), style));
            } else {
                spans.extend(cell_spans);
            }

            if c == num_cols - 1 {
                spans.push(Span::styled(" ".to_string(), style));
            }
        }
        rows.push(spans);
    }

    rows
}

/// 将 styled spans 按指定宽度换行，返回多行 spans。
/// 纯文本超宽时按字符切分，styled span 保留样式。
fn wrap_spans_to_width(spans: Vec<Span<'static>>, width: usize) -> Vec<Vec<Span<'static>>> {
    if width == 0 || spans.is_empty() {
        let text: String = spans.iter().map(|s| s.content.as_ref()).collect();
        return if text.is_empty() {
            vec![vec![]]
        } else {
            vec![spans]
        };
    }

    let total_width: usize = spans.iter().map(|s| s.content.width()).sum();
    if total_width <= width {
        return vec![spans];
    }

    let mut lines: Vec<Vec<Span<'static>>> = Vec::new();
    let mut current: Vec<Span<'static>> = Vec::new();
    let mut current_width = 0usize;

    for span in spans {
        let span_width = span.content.width();
        if current_width + span_width <= width {
            current.push(span);
            current_width += span_width;
        } else if current_width == 0 {
            // 单个 span 就超过宽度，按字符切分
            let mut buf = String::new();
            let mut buf_width = 0usize;
            for ch in span.content.chars() {
                let ch_w = UnicodeWidthChar::width(ch).unwrap_or(1);
                if buf_width + ch_w > width && !buf.is_empty() {
                    current.push(Span::styled(std::mem::take(&mut buf), span.style));
                    lines.push(std::mem::take(&mut current));
                    buf_width = 0;
                }
                buf.push(ch);
                buf_width += ch_w;
            }
            if !buf.is_empty() {
                current.push(Span::styled(buf, span.style));
                current_width = buf_width;
            }
        } else {
            // 当前行已有内容，把当前行结束，这个 span 开新行
            lines.push(std::mem::take(&mut current));
            current = vec![span];
            current_width = span_width;
        }
    }

    if !current.is_empty() || lines.is_empty() {
        lines.push(current);
    }

    lines
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

#[cfg(test)]
mod tests {
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
}
