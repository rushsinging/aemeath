use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;

/// 将纯文本中的内联 Markdown 标记转换为 styled Span 列表。
///
/// 支持的语法：
/// - `**bold**`, `__bold__` → Bold
/// - `*italic*`, `_italic_` → Italic
/// - `` `code` `` → Code (深色背景)
/// - `~~strikethrough~~` → Strikethrough
/// - `[text](url)` → 链接（青色+下划线样式）
/// - 格式不完整时原样显示原始文本
pub fn inline_markdown_spans(text: &str, base_style: Style) -> Vec<Span<'static>> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut buf = String::new();
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            // **bold**
            '*' if chars.peek() == Some(&'*') => {
                chars.next(); // consume second '*'
                match find_closing(&chars, "**") {
                    Some(inner) => {
                        flush_plain(&mut spans, &mut buf, base_style);
                        spans.push(Span::styled(
                            inner.clone(),
                            base_style.add_modifier(Modifier::BOLD),
                        ));
                        advance_chars(&mut chars, inner.chars().count() + 2);
                    }
                    None => buf.push_str("**"),
                }
            }
            // __bold__
            '_' if chars.peek() == Some(&'_') => {
                chars.next(); // consume second '_'
                match find_closing(&chars, "__") {
                    Some(inner) => {
                        flush_plain(&mut spans, &mut buf, base_style);
                        spans.push(Span::styled(
                            inner.clone(),
                            base_style.add_modifier(Modifier::BOLD),
                        ));
                        advance_chars(&mut chars, inner.chars().count() + 2);
                    }
                    None => buf.push_str("__"),
                }
            }
            // *italic*
            '*' => match find_closing(&chars, "*") {
                Some(inner) => {
                    flush_plain(&mut spans, &mut buf, base_style);
                    spans.push(Span::styled(
                        inner.clone(),
                        base_style.add_modifier(Modifier::ITALIC),
                    ));
                    advance_chars(&mut chars, inner.chars().count() + 1);
                }
                None => buf.push('*'),
            },
            // _italic_
            '_' => match find_closing(&chars, "_") {
                Some(inner) => {
                    flush_plain(&mut spans, &mut buf, base_style);
                    spans.push(Span::styled(
                        inner.clone(),
                        base_style.add_modifier(Modifier::ITALIC),
                    ));
                    advance_chars(&mut chars, inner.chars().count() + 1);
                }
                None => buf.push('_'),
            },
            // `code`
            '`' => match find_closing(&chars, "`") {
                Some(inner) => {
                    flush_plain(&mut spans, &mut buf, base_style);
                    spans.push(Span::styled(
                        inner.clone(),
                        base_style
                            .bg(Color::Rgb(40, 44, 52))
                            .fg(Color::Rgb(171, 178, 191)),
                    ));
                    advance_chars(&mut chars, inner.chars().count() + 1);
                }
                None => buf.push('`'),
            },
            // ~~strikethrough~~
            '~' if chars.peek() == Some(&'~') => {
                chars.next(); // consume second '~'
                match find_closing(&chars, "~~") {
                    Some(inner) => {
                        flush_plain(&mut spans, &mut buf, base_style);
                        spans.push(Span::styled(
                            inner.clone(),
                            base_style.add_modifier(Modifier::CROSSED_OUT),
                        ));
                        advance_chars(&mut chars, inner.chars().count() + 2);
                    }
                    None => buf.push_str("~~"),
                }
            }
            // [text](url)
            '[' => {
                let rest: String = chars.clone().collect();
                if let Some(close_bracket) = rest.find(']') {
                    if rest[close_bracket + 1..].starts_with('(') {
                        if let Some(close_paren) = rest[close_bracket + 2..].find(')') {
                            let inner = &rest[..close_bracket];
                            let _url = &rest[close_bracket + 2..close_bracket + 2 + close_paren];
                            if !inner.is_empty() && !_url.is_empty() {
                                flush_plain(&mut spans, &mut buf, base_style);
                                spans.push(Span::styled(
                                    inner.to_string(),
                                    base_style
                                        .fg(Color::Cyan)
                                        .add_modifier(Modifier::UNDERLINED),
                                ));
                                // advance: inner + "]" + "(" + url + ")"
                                advance_chars(
                                    &mut chars,
                                    inner.chars().count() + 1 + 1 + _url.chars().count() + 1,
                                );
                                continue; // consumed through the ')', keep going
                            }
                        }
                    }
                }
                // fallback: output '[' literally
                buf.push('[');
            }
            other => {
                buf.push(other);
            }
        }
    }

    flush_plain(&mut spans, &mut buf, base_style);
    spans
}

/// 在迭代器的剩余字符串中查找 closing 标记。如果找到，返回标记前的文本。
/// **不会**消耗迭代器（使用 clone 做前瞻）。
fn find_closing(chars: &std::iter::Peekable<std::str::Chars>, close: &str) -> Option<String> {
    let rest: String = chars.clone().collect();
    rest.find(close).map(|end| rest[..end].to_string())
}

/// 将迭代器前进 n 个字符
fn advance_chars(chars: &mut std::iter::Peekable<std::str::Chars>, n: usize) {
    for _ in 0..n {
        chars.next();
    }
}

/// 将累积的纯文本刷出为一个 Span
fn flush_plain(spans: &mut Vec<Span<'static>>, buf: &mut String, base_style: Style) {
    if !buf.is_empty() {
        spans.push(Span::styled(std::mem::take(buf), base_style));
    }
}

// ── Table rendering ──

/// 检测 Markdown 表格分隔行，如 `|---|---|`、`| :---: | ---: |`
pub fn is_table_separator(line: &str) -> bool {
    let trimmed = line.trim();
    if !trimmed.starts_with('|') || !trimmed.ends_with('|') || trimmed.len() <= 2 {
        return false;
    }
    let inner = trimmed[1..trimmed.len() - 1].trim();
    // 每个段必须是 :-+(-*:)? 形式
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
    let trimmed = &trimmed[1..trimmed.len() - 1]; // strip leading/trailing |
    trimmed.split('|').map(|s| s.trim()).collect()
}

/// 渲染整个表格块为 Vec<Vec<Span>>（每行一组 spans）。
///
/// - `lines`: 连续的表格行（header + separator + data rows）
/// - `base_style`: 基础文本样式
///
/// 策略：计算每列最大宽度，用 box-drawing 字符 `│` 分隔列，header 行粗体。
/// separator 行渲染为 `┼` 连接的水平线。
pub fn render_table_block(lines: &[&str], base_style: Style) -> Vec<Vec<Span<'static>>> {
    if lines.is_empty() {
        return vec![];
    }

    // 解析所有数据行（跳过 separator）
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

    // 计算每列最大宽度
    let mut col_widths = vec![0usize; num_cols];
    for row in &all_cells {
        for (c, cell) in row.iter().enumerate() {
            if c < num_cols {
                use unicode_width::UnicodeWidthStr;
                col_widths[c] = col_widths[c].max(cell.width());
            }
        }
    }

    // 构建渲染行
    let mut result = Vec::new();
    let mut data_row_idx = 0;
    let header_style = base_style.add_modifier(Modifier::BOLD);
    let border_style = base_style.fg(Color::DarkGray);

    for (i, line) in lines.iter().enumerate() {
        if is_table_separator(line) {
            // 水平分隔线：────┼────┼────
            let mut spans = Vec::new();
            for (c, &w) in col_widths.iter().enumerate() {
                if c > 0 {
                    spans.push(Span::styled("┼".to_string(), border_style));
                }
                spans.push(Span::styled("─".repeat(w + 2), border_style));
            }
            result.push(spans);
        } else {
            let cells = if data_row_idx < all_cells.len() {
                &all_cells[data_row_idx]
            } else {
                continue;
            };
            data_row_idx += 1;

            let is_header = separator_idx.map_or(true, |si| i < si);
            let style = if is_header { header_style } else { base_style };

            let mut spans = Vec::new();
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
                    let pad = w - cell_width;
                    // 右填充
                    format!("{}{}", cell, " ".repeat(pad))
                } else {
                    cell.to_string()
                };
                spans.push(Span::styled(padded, style));

                // 最后一列右侧空格
                if c == num_cols - 1 {
                    spans.push(Span::styled(" ".to_string(), style));
                }
            }
            result.push(spans);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Style {
        Style::default()
    }

    #[test]
    fn plain_text() {
        let spans = inline_markdown_spans("hello world", base());
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].content, "hello world");
    }

    #[test]
    fn bold_text() {
        let spans = inline_markdown_spans("**bold**", base());
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].content, "bold");
        assert!(spans[0].style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn italic_text() {
        let spans = inline_markdown_spans("*italic*", base());
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].content, "italic");
    }

    #[test]
    fn inline_code() {
        let spans = inline_markdown_spans("use `HashMap` here", base());
        assert_eq!(spans.len(), 3);
        assert_eq!(spans[0].content, "use ");
        assert_eq!(spans[1].content, "HashMap");
        assert_eq!(spans[2].content, " here");
    }

    #[test]
    fn mixed_formatting() {
        let spans = inline_markdown_spans("**bold** and *italic*", base());
        assert_eq!(spans.len(), 3);
        assert_eq!(spans[0].content, "bold");
        assert_eq!(spans[1].content, " and ");
        assert_eq!(spans[2].content, "italic");
    }

    #[test]
    fn strikethrough() {
        let spans = inline_markdown_spans("~~deleted~~", base());
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].content, "deleted");
    }

    #[test]
    fn unclosed_marker_outputs_literal() {
        let spans = inline_markdown_spans("**unclosed", base());
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].content, "**unclosed");
    }

    #[test]
    fn link() {
        let spans = inline_markdown_spans("click [here](https://example.com) now", base());
        assert_eq!(spans.len(), 3);
        assert_eq!(spans[0].content, "click ");
        assert_eq!(spans[1].content, "here");
        assert_eq!(spans[2].content, " now");
    }

    #[test]
    fn empty_string() {
        let spans = inline_markdown_spans("", base());
        assert_eq!(spans.len(), 0);
    }

    #[test]
    fn multiple_bold_spans() {
        let spans = inline_markdown_spans("**a** and **b**", base());
        assert_eq!(spans.len(), 3);
        assert_eq!(spans[0].content, "a");
        assert_eq!(spans[1].content, " and ");
        assert_eq!(spans[2].content, "b");
    }

    // ── Table tests ──

    #[test]
    fn table_separator_detection() {
        assert!(is_table_separator("|---|---|"));
        assert!(is_table_separator("| --- | --- |"));
        assert!(is_table_separator("|:---|:---:|---:|"));
        assert!(is_table_separator("| --- | :---: | ---: |"));
        assert!(!is_table_separator("| hello | world |"));
        assert!(!is_table_separator("just text"));
    }

    #[test]
    fn table_row_detection() {
        assert!(is_table_row("| hello | world |"));
        assert!(is_table_row("| a | b | c |"));
        assert!(!is_table_row("just text"));
        assert!(!is_table_row("|---|---|"));
    }

    #[test]
    fn test_parse_table_cells() {
        assert_eq!(
            parse_table_cells("| hello | world |"),
            vec!["hello", "world"]
        );
        assert_eq!(parse_table_cells("| a | b | c |"), vec!["a", "b", "c"]);
        assert_eq!(parse_table_cells("| single |"), vec!["single"]);
    }

    #[test]
    fn test_render_simple_table() {
        let base = Style::default().fg(Color::Green);
        let lines = &["| Name | Value |", "| --- | --- |", "| foo  | bar   |"];
        let rendered = render_table_block(lines, base);
        assert_eq!(rendered.len(), 3);
        // header should be bold
        let header_spans = &rendered[0];
        assert!(header_spans.iter().any(|s| s.content.contains("Name")));
    }
}
