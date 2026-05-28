//! 分段着色行渲染辅助函数（用于测试 slice_spans）。

use crate::tui::output_area::SpanPart;
use ratatui::style::Color;

/// 按字符偏移和长度从 SpanPart 列表中切出子片段，用于换行渲染。
/// 返回 (text, color) 对的列表。
#[allow(dead_code)]
pub fn slice_spans(
    spans: &[SpanPart],
    char_offset: usize,
    char_count: usize,
) -> Vec<(String, Color)> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::output_area::SpanPart;

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
