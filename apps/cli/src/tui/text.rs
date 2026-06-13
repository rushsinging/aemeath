use std::ops::Range;

use unicode_width::UnicodeWidthChar;

pub fn clamp_char_range(from: usize, to: usize, chars_len: usize) -> Option<Range<usize>> {
    let from = from.min(chars_len);
    let to = to.min(chars_len);
    if from >= to {
        None
    } else {
        Some(from..to)
    }
}

pub fn safe_char_slice(chars: &[char], from: usize, to: usize) -> &[char] {
    match clamp_char_range(from, to, chars.len()) {
        Some(range) => &chars[range],
        None => &[],
    }
}

pub fn safe_str_slice_by_char(s: &str, from: usize, to: usize) -> &str {
    let char_len = s.chars().count();
    let Some(range) = clamp_char_range(from, to, char_len) else {
        return "";
    };
    let byte_start = char_to_byte_clamped(s, range.start);
    let byte_end = char_to_byte_clamped(s, range.end);
    &s[byte_start..byte_end] // allow unsafe_text_op: byte_start/byte_end are computed from char_indices boundaries
}

pub fn truncate_unicode_width(s: &str, max_cols: usize) -> (&str, usize) {
    if max_cols == 0 {
        return ("", 0);
    }

    let total_width = str_display_width(s);
    if total_width <= max_cols {
        return (s, total_width);
    }

    let mut width = 0usize;
    let mut end = 0usize;
    for (byte_idx, ch) in s.char_indices() {
        let ch_width = char_display_width(ch);
        if width + ch_width > max_cols {
            break;
        }
        width += ch_width;
        end = byte_idx + ch.len_utf8();
    }
    (&s[..end], width) // allow unsafe_text_op: end is computed from char_indices plus len_utf8
}

pub fn str_display_width(s: &str) -> usize {
    s.chars().map(char_display_width).sum()
}

pub fn col_to_char_idx(s: &str, col: usize) -> usize {
    let mut width = 0usize;
    for (char_idx, ch) in s.chars().enumerate() {
        // Control and zero-width chars do not advance TUI display columns.
        let ch_width = char_display_width(ch);
        if width + ch_width > col {
            return char_idx;
        }
        width += ch_width;
    }
    s.chars().count()
}

pub fn clamp_split_index(offset: usize, len: usize) -> usize {
    offset.min(len)
}

pub fn safe_byte_prefix(s: &str, offset: usize) -> &str {
    let mut offset = offset.min(s.len());
    while offset > 0 && !s.is_char_boundary(offset) {
        offset -= 1;
    }
    &s[..offset] // allow unsafe_text_op: offset is clamped backward to a char boundary
}

fn char_display_width(ch: char) -> usize {
    ch.width().unwrap_or(0)
}

fn char_to_byte_clamped(s: &str, char_idx: usize) -> usize {
    s.char_indices()
        .nth(char_idx)
        .map(|(byte_idx, _)| byte_idx)
        .unwrap_or(s.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clamp_char_range_normal() {
        assert_eq!(clamp_char_range(1, 3, 5), Some(1..3));
    }

    #[test]
    fn test_clamp_char_range_empty_or_reversed() {
        assert_eq!(clamp_char_range(2, 2, 5), None);
        assert_eq!(clamp_char_range(4, 2, 5), None);
    }

    #[test]
    fn test_clamp_char_range_out_of_bounds() {
        assert_eq!(clamp_char_range(1, 99, 4), Some(1..4));
        assert_eq!(clamp_char_range(99, 100, 4), None);
    }

    #[test]
    fn test_safe_char_slice_ascii() {
        let chars: Vec<char> = "hello".chars().collect();
        assert_eq!(safe_char_slice(&chars, 1, 4), &['e', 'l', 'l']);
    }

    #[test]
    fn test_safe_char_slice_cjk_and_emoji() {
        let chars: Vec<char> = "你🚀好".chars().collect();
        assert_eq!(safe_char_slice(&chars, 0, 2), &['你', '🚀']);
        assert_eq!(safe_char_slice(&chars, 2, 99), &['好']);
    }

    #[test]
    fn test_safe_char_slice_invalid_range_returns_empty() {
        let chars: Vec<char> = "abc".chars().collect();
        assert!(safe_char_slice(&chars, 3, 3).is_empty());
        assert!(safe_char_slice(&chars, 9, 10).is_empty());
        assert!(safe_char_slice(&chars, 2, 1).is_empty());
    }

    #[test]
    fn test_safe_char_slice_empty_slice_returns_empty() {
        let chars: Vec<char> = Vec::new();
        assert!(safe_char_slice(&chars, 0, 0).is_empty());
        assert!(safe_char_slice(&chars, 0, 1).is_empty());
    }

    #[test]
    fn test_safe_str_slice_by_char_ascii() {
        assert_eq!(safe_str_slice_by_char("hello", 1, 4), "ell");
        assert_eq!(safe_str_slice_by_char("hello", 1, 5), "ello");
    }

    #[test]
    fn test_safe_str_slice_by_char_empty_string_returns_empty() {
        assert_eq!(safe_str_slice_by_char("", 0, 0), "");
        assert_eq!(safe_str_slice_by_char("", 0, 1), "");
    }

    #[test]
    fn test_safe_str_slice_by_char_utf8_boundaries() {
        assert_eq!(safe_str_slice_by_char("你🚀好", 0, 2), "你🚀");
        assert_eq!(safe_str_slice_by_char("你🚀好", 2, 99), "好");
    }

    #[test]
    fn test_safe_str_slice_by_char_invalid_range_returns_empty() {
        assert_eq!(safe_str_slice_by_char("abc", 2, 1), "");
        assert_eq!(safe_str_slice_by_char("abc", 9, 10), "");
    }

    #[test]
    fn test_truncate_unicode_width_ascii() {
        assert_eq!(truncate_unicode_width("hello", 3), ("hel", 3));
        assert_eq!(truncate_unicode_width("hi", 3), ("hi", 2));
    }

    #[test]
    fn test_truncate_unicode_width_cjk() {
        assert_eq!(truncate_unicode_width("你好世界", 4), ("你好", 4));
        assert_eq!(truncate_unicode_width("你好", 1), ("", 0));
    }

    #[test]
    fn test_truncate_unicode_width_emoji() {
        assert_eq!(truncate_unicode_width("a🚀b", 3), ("a🚀", 3));
        assert_eq!(truncate_unicode_width("a🚀b", 2), ("a", 1));
    }

    #[test]
    fn test_truncate_unicode_width_empty_string() {
        assert_eq!(truncate_unicode_width("", 0), ("", 0));
        assert_eq!(truncate_unicode_width("", 3), ("", 0));
    }

    #[test]
    fn test_truncate_unicode_width_control_and_zero_width() {
        assert_eq!(truncate_unicode_width("a\u{0000}b", 2), ("a\u{0000}b", 2));
        assert_eq!(truncate_unicode_width("a\u{0301}b", 2), ("a\u{0301}b", 2));
    }

    #[test]
    fn test_str_display_width_control_and_zero_width() {
        assert_eq!(str_display_width("a\u{0000}b"), 2);
        assert_eq!(str_display_width("a\u{0301}b"), 2);
    }

    #[test]
    fn test_col_to_char_idx_ascii_cjk_emoji() {
        assert_eq!(col_to_char_idx("hello", 2), 2);
        assert_eq!(col_to_char_idx("你好", 1), 0);
        assert_eq!(col_to_char_idx("你好", 2), 1);
        assert_eq!(col_to_char_idx("a🚀b", 2), 1);
        assert_eq!(col_to_char_idx("a🚀b", 99), 3);
    }

    #[test]
    fn test_col_to_char_idx_empty_string() {
        assert_eq!(col_to_char_idx("", 0), 0);
        assert_eq!(col_to_char_idx("", 3), 0);
    }

    #[test]
    fn test_col_to_char_idx_control_and_zero_width() {
        assert_eq!(col_to_char_idx("a\u{0000}b", 1), 2);
        assert_eq!(col_to_char_idx("a\u{0301}b", 1), 2);
    }

    #[test]
    fn test_safe_byte_prefix_clamps_to_char_boundary() {
        assert_eq!(safe_byte_prefix("a🚀b", 0), "");
        assert_eq!(safe_byte_prefix("a🚀b", 1), "a");
        assert_eq!(safe_byte_prefix("a🚀b", 2), "a");
        assert_eq!(safe_byte_prefix("a🚀b", 5), "a🚀");
        assert_eq!(safe_byte_prefix("a🚀b", 99), "a🚀b");
    }

    #[test]
    fn test_clamp_split_index() {
        assert_eq!(clamp_split_index(0, 3), 0);
        assert_eq!(clamp_split_index(2, 3), 2);
        assert_eq!(clamp_split_index(9, 3), 3);
    }
}
