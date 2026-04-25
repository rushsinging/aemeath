use aemeath_core::string_idx::CharIdx;
use unicode_width::UnicodeWidthChar;

/// Truncate a string to fit within `max_width` Unicode display columns,
/// appending "..." if truncated.
pub fn truncate_unicode_width(s: &str, max_width: usize) -> String {
    use unicode_width::UnicodeWidthStr;
    let width = s.width();
    if width <= max_width {
        return s.to_string();
    }
    if max_width <= 3 {
        return "...".chars().take(max_width).collect();
    }
    let target = max_width - 3;
    let mut end = 0;
    let mut w = 0;
    for (i, ch) in s.char_indices() {
        let cw = ch.width().unwrap_or(0);
        if w + cw > target {
            break;
        }
        w += cw;
        end = i + ch.len_utf8();
    }
    format!("{}...", &s[..end])
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
    let mut display_width = 0usize;
    for (char_idx, ch) in text.chars().enumerate() {
        let ch_w = ch.width().unwrap_or(1) as usize;
        if display_width + ch_w > screen_col {
            return CharIdx::new(char_idx);
        }
        display_width += ch_w;
    }
    CharIdx::new(text.chars().count())
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
