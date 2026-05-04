use std::ops::Range;

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

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
    &s[byte_start..byte_end]
}

pub fn safe_char_at(s: &str, idx: usize) -> Option<char> {
    s.chars().nth(idx)
}

pub fn truncate_unicode_width(s: &str, max_cols: usize) -> (&str, usize) {
    if max_cols == 0 {
        return ("", 0);
    }
    if s.width() <= max_cols {
        return (s, s.width());
    }

    let mut width = 0usize;
    let mut end = 0usize;
    for (byte_idx, ch) in s.char_indices() {
        let ch_width = ch.width().unwrap_or(0);
        if width + ch_width > max_cols {
            break;
        }
        width += ch_width;
        end = byte_idx + ch.len_utf8();
    }
    (&s[..end], width)
}

pub fn col_to_char_idx(s: &str, col: usize) -> usize {
    let mut width = 0usize;
    for (char_idx, ch) in s.chars().enumerate() {
        let ch_width = ch.width().unwrap_or(1);
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
    fn test_safe_str_slice_by_char_ascii() {
        assert_eq!(safe_str_slice_by_char("hello", 1, 4), "ell");
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
    fn test_safe_char_at_bounds() {
        assert_eq!(safe_char_at("你a", 0), Some('你'));
        assert_eq!(safe_char_at("你a", 1), Some('a'));
        assert_eq!(safe_char_at("你a", 2), None);
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
    fn test_col_to_char_idx_ascii_cjk_emoji() {
        assert_eq!(col_to_char_idx("hello", 2), 2);
        assert_eq!(col_to_char_idx("你好", 2), 1);
        assert_eq!(col_to_char_idx("a🚀b", 2), 1);
        assert_eq!(col_to_char_idx("a🚀b", 99), 3);
    }

    #[test]
    fn test_clamp_split_index() {
        assert_eq!(clamp_split_index(0, 3), 0);
        assert_eq!(clamp_split_index(2, 3), 2);
        assert_eq!(clamp_split_index(9, 3), 3);
    }
}
