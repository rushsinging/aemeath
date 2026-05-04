use aemeath_core::string_idx::CharIdx;
use unicode_width::UnicodeWidthChar;

use crate::tui::safe_text;

/// Truncate a string to fit within `max_width` Unicode display columns,
/// appending "..." if truncated.
pub fn truncate_unicode_width(s: &str, max_width: usize) -> String {
    let (_, total_width) = safe_text::truncate_unicode_width(s, usize::MAX);
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
