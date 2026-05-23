//! 分段着色行渲染（如 diff 语法高亮 + 行号）。

use ratatui::{
    style::Style,
    text::{Line, Span},
};

use aemeath_core::string_idx::CharIdx;

use super::types::SpanPart;
use super::OutputArea;

impl OutputArea {
    /// 渲染带分段着色的行（如 diff 语法高亮 + 行号）。
    pub(super) fn render_span_line(
        &self,
        idx: usize,
        span_parts: &[SpanPart],
        screen_map: &mut Vec<(usize, CharIdx, CharIdx)>,
        lines: &mut Vec<Line<'static>>,
    ) {
        // 将 SpanPart 转为纯文本用于换行计算
        let full_text: String = span_parts.iter().map(|s| s.text.as_str()).collect();
        let wrapped = self.push_wrapped_offsets(idx, &full_text, screen_map);

        if wrapped.len() <= 1 {
            // 单行：直接构建 spans
            let ratatui_spans: Vec<Span> = span_parts
                .iter()
                .map(|sp| Span::styled(sp.text.clone(), Style::default().fg(sp.color)))
                .collect();
            lines.push(Line::from(ratatui_spans));
        } else {
            // 多行换行：按字符偏移切分 spans 到每个 chunk
            let mut char_offset = 0usize;
            for chunk in &wrapped {
                let chunk_char_count = chunk.chars().count();
                let chunk_spans = slice_spans(span_parts, char_offset, chunk_char_count);
                let ratatui_spans: Vec<Span> = chunk_spans
                    .into_iter()
                    .map(|(text, color)| Span::styled(text, Style::default().fg(color)))
                    .collect();
                lines.push(Line::from(ratatui_spans));
                char_offset += chunk_char_count;
            }
        }
    }
}

/// 按字符偏移和长度从 SpanPart 列表中切出子片段，用于换行渲染。
/// 返回 (text, color) 对的列表。
fn slice_spans(
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

        // 计算交集
        let slice_start = sp_start.max(char_offset);
        let slice_end = sp_end.min(end);

        if slice_start < slice_end {
            // 按字符偏移切取子串
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::output_area::SpanPart;
    use ratatui::style::Color;

    #[test]
    fn test_slice_spans_single_span_full() {
        let spans = vec![SpanPart::plain("hello", Color::Red)];
        let result = slice_spans(&spans, 0, 5);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, "hello");
    }

    #[test]
    fn test_slice_spans_multiple_spans_partial() {
        let spans = vec![
            SpanPart::plain("abc", Color::Red),
            SpanPart::plain("def", Color::Green),
        ];
        let result = slice_spans(&spans, 2, 3);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].0, "c");
        assert_eq!(result[1].0, "de");
    }

    #[test]
    fn test_slice_spans_out_of_range() {
        let spans = vec![SpanPart::plain("hi", Color::Red)];
        let result = slice_spans(&spans, 5, 2);
        assert!(result.is_empty());
    }

    #[test]
    fn test_slice_spans_exact_boundary() {
        let spans = vec![
            SpanPart::plain("ab", Color::Red),
            SpanPart::plain("cd", Color::Green),
        ];
        let result = slice_spans(&spans, 2, 2);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, "cd");
    }
}
