//! 渲染缓存行渲染函数：从 render_range 拆出。

use std::collections::HashMap;

use ratatui::style::Style;
use ratatui::text::{Line, Span};

use aemeath_core::string_idx::CharIdx;

use super::display;
use super::markdown;
use super::render::wrap_line;
use super::types::{LineStyle, SpanPart};
use super::OutputLine;
use crate::tui::theme;

use super::rendered_cache::RenderedLine;

/// 渲染 [start, end) 范围内的所有行，写入 cache。
pub(super) fn render_range(
    lines: &[OutputLine],
    start: usize,
    end: usize,
    term_width: usize,
    cache: &mut [Option<RenderedLine>],
) {
    if start >= end || term_width == 0 {
        return;
    }

    // 先扫描 block 状态到 start
    let mut in_code_block = false;
    for line in lines.iter().take(start) {
        if is_markdown_style(line.style) && line.content.trim().starts_with("```") {
            in_code_block = !in_code_block;
        }
    }
    let table_ranges = collect_table_ranges(lines, start, end);
    let code_style = Style::default().fg(theme::CODE);

    let mut i = start;
    while i < end {
        if let Some(&(ts, te)) = table_ranges.get(&i) {
            render_table_range(lines, ts, te, term_width, cache);
            i = te;
            continue;
        }

        let line = &lines[i];
        let is_md = is_markdown_style(line.style);
        let trimmed = line.content.trim();

        if is_md && trimmed.starts_with("```") {
            let lang = if !in_code_block {
                trimmed.strip_prefix("```").unwrap_or("").trim().to_string()
            } else {
                String::new()
            };
            in_code_block = !in_code_block;
            let border_style = Style::default().fg(theme::BORDER);
            let label = if !lang.is_empty() {
                format!("── {} ", lang)
            } else {
                "─".repeat(term_width.max(1))
            };
            let label = display::truncate_unicode_width(&label, term_width);
            let char_count = label.chars().count();
            cache[i] = Some(RenderedLine {
                line: Line::styled(label, border_style),
                screen_entries: vec![(i, CharIdx::ZERO, CharIdx::new(char_count))],
                rendered_text: None,
            });
            i += 1;
            continue;
        }

        if let Some(ref span_parts) = line.spans {
            render_span_line(i, span_parts, term_width, cache);
        } else if in_code_block {
            render_plain_line(i, &line.content, code_style, term_width, cache);
        } else if is_md {
            render_markdown_line(i, line, term_width, cache);
        } else {
            render_plain_line(i, &line.content, line.style.to_style(), term_width, cache);
        }

        i += 1;
    }
}

fn collect_table_ranges(
    lines: &[OutputLine],
    start: usize,
    end: usize,
) -> HashMap<usize, (usize, usize)> {
    let mut ranges = HashMap::new();
    let mut i = start;

    while i < end {
        let line = &lines[i];
        let is_md = is_markdown_style(line.style);
        let trimmed = line.content.trim();

        if is_md && (markdown::is_table_row(trimmed) || markdown::is_table_separator(trimmed)) {
            let block_start = i;
            let mut block_end = i + 1;
            while block_end < end {
                let next = &lines[block_end];
                let next_md = is_markdown_style(next.style);
                let t = next.content.trim();
                if next_md && (markdown::is_table_row(t) || markdown::is_table_separator(t)) {
                    block_end += 1;
                } else {
                    break;
                }
            }
            let has_sep = (block_start..block_end)
                .any(|j| markdown::is_table_separator(lines[j].content.trim()));
            if has_sep {
                ranges.insert(block_start, (block_start, block_end));
            }
            i = block_end;
        } else {
            i += 1;
        }
    }

    ranges
}

fn render_table_range(
    lines: &[OutputLine],
    table_start: usize,
    table_end: usize,
    term_width: usize,
    cache: &mut [Option<RenderedLine>],
) {
    let table_lines: Vec<&str> = (table_start..table_end)
        .map(|i| lines[i].content.trim())
        .collect();

    let base_style = lines[table_start].style.to_style();
    let rendered_rows = markdown::render_table_block(&table_lines, base_style, term_width);

    let mut row_idx = 0;
    let mut i = table_start;
    while i < table_end && row_idx < rendered_rows.len() {
        let trimmed = lines[i].content.trim();
        if markdown::is_table_separator(trimmed) {
            let spans = &rendered_rows[row_idx];
            let text: String = spans.iter().map(|s| s.content.as_ref()).collect();
            cache[i] = Some(RenderedLine {
                line: Line::from(spans.clone()),
                screen_entries: vec![(i, CharIdx::ZERO, CharIdx::new(text.chars().count()))],
                rendered_text: Some(text),
            });
            row_idx += 1;
            i += 1;
        } else {
            let mut sub_rows = vec![rendered_rows[row_idx].clone()];
            row_idx += 1;

            let next_is_sep =
                (i + 1) < table_end && markdown::is_table_separator(lines[i + 1].content.trim());
            let next_is_data =
                (i + 1) < table_end && markdown::is_table_row(lines[i + 1].content.trim());

            if !next_is_sep && !next_is_data {
                while row_idx < rendered_rows.len() {
                    sub_rows.push(rendered_rows[row_idx].clone());
                    row_idx += 1;
                }
            }

            let full_text: String = sub_rows
                .iter()
                .flat_map(|r| r.iter().map(|s| s.content.as_ref()))
                .collect::<Vec<&str>>()
                .join("");
            let mut screen_entries = Vec::new();
            let mut char_offset = 0usize;

            for sub in &sub_rows {
                let line_text: String = sub.iter().map(|s| s.content.as_ref()).collect();
                let char_count = line_text.chars().count();
                screen_entries.push((
                    i,
                    CharIdx::new(char_offset),
                    CharIdx::new(char_offset + char_count),
                ));
                char_offset += char_count;
            }

            if sub_rows.len() == 1 {
                cache[i] = Some(RenderedLine {
                    line: Line::from(sub_rows.into_iter().next().unwrap()),
                    screen_entries,
                    rendered_text: Some(full_text),
                });
            } else {
                let all_spans: Vec<Span> = sub_rows
                    .iter()
                    .flat_map(|r| {
                        let mut s = r.clone();
                        s.push(Span::raw("\n"));
                        s
                    })
                    .collect();
                cache[i] = Some(RenderedLine {
                    line: Line::from(all_spans),
                    screen_entries,
                    rendered_text: Some(full_text),
                });
            }

            i += 1;
        }
    }
}

fn render_markdown_line(
    idx: usize,
    line: &OutputLine,
    term_width: usize,
    cache: &mut [Option<RenderedLine>],
) {
    let rendered_plain = markdown::strip_inline_formatting(&line.content);
    let rendered_text = if rendered_plain != line.content {
        Some(rendered_plain.clone())
    } else {
        None
    };

    let md_lines =
        markdown::inline_markdown_lines(&line.content, line.style.to_style(), term_width);

    let mut screen_entries = Vec::new();
    let mut char_offset = 0usize;
    for md_line in &md_lines {
        let text: String = md_line.spans.iter().map(|s| s.content.as_ref()).collect();
        let char_count = text.chars().count();
        screen_entries.push((
            idx,
            CharIdx::new(char_offset),
            CharIdx::new(char_offset + char_count),
        ));
        char_offset += char_count;
    }
    if md_lines.len() == 1 {
        cache[idx] = Some(RenderedLine {
            line: md_lines.into_iter().next().unwrap(),
            screen_entries,
            rendered_text,
        });
    } else {
        let all_spans: Vec<Span> = md_lines
            .into_iter()
            .flat_map(|l| {
                let mut spans = l.spans;
                spans.push(Span::raw("\n"));
                spans
            })
            .collect();
        cache[idx] = Some(RenderedLine {
            line: Line::from(all_spans),
            screen_entries,
            rendered_text,
        });
    }
}

fn render_plain_line(
    idx: usize,
    content: &str,
    style: Style,
    term_width: usize,
    cache: &mut [Option<RenderedLine>],
) {
    let sanitized = display::sanitize_for_display(content);
    let char_offsets = display::compute_char_offsets(&sanitized, term_width);
    let wrapped = wrap_line(content, term_width);

    let mut screen_entries = Vec::new();
    for (chunk_idx, _) in wrapped.iter().enumerate() {
        let (char_start, char_end) = char_offsets
            .get(chunk_idx)
            .copied()
            .unwrap_or((CharIdx::ZERO, CharIdx::ZERO));
        screen_entries.push((idx, char_start, char_end));
    }

    if wrapped.len() == 1 {
        cache[idx] = Some(RenderedLine {
            line: Line::styled(wrapped.into_iter().next().unwrap(), style),
            screen_entries,
            rendered_text: None,
        });
    } else {
        let all_spans: Vec<Span> = wrapped
            .into_iter()
            .flat_map(|chunk| vec![Span::styled(chunk, style), Span::raw("\n")])
            .collect();
        cache[idx] = Some(RenderedLine {
            line: Line::from(all_spans),
            screen_entries,
            rendered_text: None,
        });
    }
}

fn render_span_line(
    idx: usize,
    span_parts: &[SpanPart],
    term_width: usize,
    cache: &mut [Option<RenderedLine>],
) {
    let full_text: String = span_parts.iter().map(|s| s.text.as_str()).collect();
    let sanitized = display::sanitize_for_display(&full_text);
    let char_offsets = display::compute_char_offsets(&sanitized, term_width);
    let wrapped = wrap_line(&full_text, term_width);

    let mut screen_entries = Vec::new();
    for (chunk_idx, _) in wrapped.iter().enumerate() {
        let (char_start, char_end) = char_offsets
            .get(chunk_idx)
            .copied()
            .unwrap_or((CharIdx::ZERO, CharIdx::ZERO));
        screen_entries.push((idx, char_start, char_end));
    }

    let all_spans: Vec<Span> = if wrapped.len() <= 1 {
        span_parts
            .iter()
            .map(|sp| Span::styled(sp.text.clone(), Style::default().fg(sp.color)))
            .collect()
    } else {
        let mut char_offset = 0usize;
        let mut spans = Vec::new();
        for chunk in &wrapped {
            let chunk_char_count = chunk.chars().count();
            let chunk_spans = slice_spans_impl(span_parts, char_offset, chunk_char_count);
            for (text, color) in chunk_spans {
                spans.push(Span::styled(text, color));
            }
            spans.push(Span::raw("\n"));
            char_offset += chunk_char_count;
        }
        spans
    };

    cache[idx] = Some(RenderedLine {
        line: Line::from(all_spans),
        screen_entries,
        rendered_text: None,
    });
}

fn slice_spans_impl(
    spans: &[SpanPart],
    char_offset: usize,
    char_count: usize,
) -> Vec<(String, ratatui::style::Color)> {
    let mut result = Vec::new();
    let mut current_offset = 0usize;
    let end = char_offset + char_count;

    for sp in spans {
        let sp_len = sp.text.chars().count();
        let sp_start = current_offset;
        let sp_end = sp_start + sp_len;

        if sp_end <= char_offset || sp_start >= end {
            current_offset += sp_len;
            continue;
        }

        let slice_start = sp_start.max(char_offset);
        let slice_end = sp_end.min(end);

        if slice_start < slice_end {
            let skip = slice_start.saturating_sub(sp_start);
            let take = slice_end - slice_start;
            let sub_text: String = sp.text.chars().skip(skip).take(take).collect();
            if !sub_text.is_empty() {
                result.push((sub_text, sp.color));
            }
        }

        current_offset += sp_len;
    }

    result
}

pub(super) fn is_markdown_style(style: LineStyle) -> bool {
    matches!(
        style,
        LineStyle::Assistant | LineStyle::Thinking | LineStyle::System
    )
}
