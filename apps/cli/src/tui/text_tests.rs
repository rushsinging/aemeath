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
fn test_truncate_last_unicode_width_ascii() {
    assert_eq!(truncate_last_unicode_width("hello", 3), ("llo", 3));
    assert_eq!(truncate_last_unicode_width("hi", 3), ("hi", 2));
}

#[test]
fn test_truncate_last_unicode_width_cjk_and_emoji() {
    // 中文每字 2 列：max_cols=4 只能容纳末尾两个字，且落在 char 边界。
    assert_eq!(truncate_last_unicode_width("你好世界", 4), ("世界", 4));
    // 末字符为 2 列 emoji，max_cols=1 容不下 → 空，不切进字符内部。
    assert_eq!(truncate_last_unicode_width("a🚀", 1), ("", 0));
    assert_eq!(truncate_last_unicode_width("a🚀b", 3), ("🚀b", 3));
}

#[test]
fn test_truncate_last_unicode_width_zero_and_empty() {
    assert_eq!(truncate_last_unicode_width("你好", 0), ("", 0));
    assert_eq!(truncate_last_unicode_width("", 3), ("", 0));
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

#[test]
fn test_split_at_ascii_whitespace() {
    let (indent, rest) = split_at_ascii("  hello", |c| c.is_ascii_whitespace());
    assert_eq!(indent, "  ");
    assert_eq!(rest, "hello");
}

#[test]
fn test_split_at_ascii_digits() {
    let (digits, rest) = split_at_ascii("123abc", |c| c.is_ascii_digit());
    assert_eq!(digits, "123");
    assert_eq!(rest, "abc");
}

#[test]
fn test_split_at_ascii_no_match() {
    let (prefix, rest) = split_at_ascii("hello", |c| c.is_ascii_digit());
    assert_eq!(prefix, "");
    assert_eq!(rest, "hello");
}

#[test]
fn test_split_at_ascii_all_match() {
    let (prefix, rest) = split_at_ascii("123", |c| c.is_ascii_digit());
    assert_eq!(prefix, "123");
    assert_eq!(rest, "");
}

#[test]
fn test_split_at_ascii_empty() {
    let (prefix, rest) = split_at_ascii("", |c| c.is_ascii_whitespace());
    assert_eq!(prefix, "");
    assert_eq!(rest, "");
}
