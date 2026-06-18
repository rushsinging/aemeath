use sdk::CharIdx;
use unicode_width::UnicodeWidthChar;

use crate::tui::render::display::safe_text;

/// Truncate a string to fit within `max_width` Unicode display columns,
/// appending "..." if truncated.
pub fn truncate_unicode_width(s: &str, max_width: usize) -> String {
    let total_width = safe_text::str_display_width(s);
    if total_width <= max_width {
        return s.to_string();
    }
    if max_width <= 3 {
        return "...".chars().take(max_width).collect();
    }
    let target = max_width - 3;
    let (prefix, _) = safe_text::truncate_unicode_width(s, target);
    format!("{prefix}...")
}

/// Sanitize a string for TUI display: expand tabs, strip ANSI escapes and control characters.
pub fn sanitize_for_display(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            '\t' => result.push_str("    "),
            '\x1b' => {
                if chars.peek() == Some(&'[') {
                    chars.next();
                    while let Some(&c) = chars.peek() {
                        chars.next();
                        if c.is_ascii_alphabetic() {
                            break;
                        }
                    }
                }
            }
            '\r' => {}
            c if c.is_control() => {}
            c => result.push(c),
        }
    }
    result
}

/// Convert a screen column position (display column) to a char index within the string.
pub fn screen_col_to_char_idx(text: &str, screen_col: usize) -> CharIdx {
    CharIdx::new(safe_text::col_to_char_idx(text, screen_col))
}

/// Split a string into lines that fit within `max_width` display columns.
pub fn wrap_line(text: &str, max_width: usize) -> Vec<String> {
    let text = sanitize_for_display(text);

    if max_width == 0 {
        return vec![text];
    }

    let mut result = Vec::new();
    let mut current = String::new();
    let mut current_width = 0usize;

    for ch in text.chars() {
        let ch_width = ch.width().unwrap_or(1);

        if current_width + ch_width > max_width {
            result.push(std::mem::take(&mut current));
            current_width = 0;
        }

        current.push(ch);
        current_width += ch_width;
    }

    if !current.is_empty() || result.is_empty() {
        result.push(current);
    }

    result
}

/// 计算 wrap 后每个 chunk 在原始文本中的 char 偏移 (start, end)
pub fn compute_char_offsets(text: &str, max_width: usize) -> Vec<(CharIdx, CharIdx)> {
    if max_width == 0 {
        let len = text.chars().count();
        return vec![(CharIdx::ZERO, CharIdx::new(len))];
    }

    let mut result = Vec::new();
    let mut current_width = 0usize;
    let mut chunk_start = 0usize;

    for (char_idx, ch) in text.chars().enumerate() {
        let ch_width = ch.width().unwrap_or(1);
        if current_width + ch_width > max_width {
            result.push((CharIdx::new(chunk_start), CharIdx::new(char_idx)));
            chunk_start = char_idx;
            current_width = 0;
        }
        current_width += ch_width;
    }

    let end = text.chars().count();
    result.push((CharIdx::new(chunk_start), CharIdx::new(end)));
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_unicode_width_regression() {
        assert_eq!(truncate_unicode_width("hello", 3), "...");
        assert_eq!(truncate_unicode_width("hello", 4), "h...");
        assert_eq!(truncate_unicode_width("你好世界", 5), "你...");
    }

    #[test]
    fn test_screen_col_to_char_idx_regression() {
        assert_eq!(screen_col_to_char_idx("a🚀b", 0), CharIdx::new(0));
        assert_eq!(screen_col_to_char_idx("a🚀b", 1), CharIdx::new(1));
        assert_eq!(screen_col_to_char_idx("a🚀b", 3), CharIdx::new(2));
    }

    /// 回归 #196：`sanitize_for_display` 必须把 `\t` 展开为 4 空格、剥离 ANSI 与控制字符，
    /// 避免 ratatui `Buffer::set_stringn` 把 `\t` 当控制字符过滤造成的列宽不一致。
    #[test]
    fn test_sanitize_for_display_expands_tabs_and_strips_control_chars() {
        assert_eq!(sanitize_for_display("a\tb"), "a    b");
        assert_eq!(
            sanitize_for_display("col1\tcol2\tcol3"),
            "col1    col2    col3"
        );
        // ANSI CSI 序列（含 ESC + '[' + 参数 + 终止字母）整体剥离。
        assert_eq!(sanitize_for_display("\x1b[31mred\x1b[0m"), "red");
        // \r 单独被跳过；\n 走 is_control 分支也跳过（实际在 TUI 渲染前是预期行为）。
        assert_eq!(sanitize_for_display("hi\r\nworld\x07"), "hiworld");
    }
}
