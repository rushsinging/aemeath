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
    s.get(byte_start..byte_end).unwrap_or("")
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
    (s.get(..end).unwrap_or(""), width)
}

/// 保留 `s` 末尾、显示宽度不超过 `max_cols` 的最长后缀（char 边界安全）。
/// 返回 (后缀, 后缀显示宽度)。与 `truncate_unicode_width` 对称，用于尾部截断。
pub fn truncate_last_unicode_width(s: &str, max_cols: usize) -> (&str, usize) {
    if max_cols == 0 {
        return ("", 0);
    }

    let total_width = str_display_width(s);
    if total_width <= max_cols {
        return (s, total_width);
    }

    let mut width = 0usize;
    let mut start = s.len();
    for (byte_idx, ch) in s.char_indices().rev() {
        let ch_width = char_display_width(ch);
        if width + ch_width > max_cols {
            break;
        }
        width += ch_width;
        start = byte_idx;
    }
    (s.get(start..).unwrap_or(""), width)
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
    s.get(..offset).unwrap_or("")
}

/// 在 ASCII 前缀边界处安全切分字符串。
///
/// 返回 (前缀, 剩余部分)。前缀由满足谓词的连续 ASCII 字符组成。
/// 由于只计数字节值 < 128 的 ASCII 字符，返回的字节偏移一定是 char 边界。
///
/// # Examples
/// ```
/// let (indent, rest) = split_at_ascii("  hello", |c| c.is_ascii_whitespace());
/// assert_eq!(indent, "  ");
/// assert_eq!(rest, "hello");
/// ```
pub fn split_at_ascii<F: Fn(char) -> bool>(s: &str, predicate: F) -> (&str, &str) {
    let byte_len = s.bytes().take_while(|&b| predicate(b as char)).count();
    // byte_len 由 ASCII 前缀逐字节计数得出，必然落在 char 边界。
    s.split_at(byte_len) // allow unsafe_text_op
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
#[path = "text_tests.rs"]
mod tests;
