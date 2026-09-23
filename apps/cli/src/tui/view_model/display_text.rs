//! 显示文本控制字符归一化策略（单一真相，issue #1670）。
//!
//! 终端渲染链路对控制字符的处理约定：
//! - `\t` 展开为 4 空格（沿用 tool result 既有约定，#196）；
//! - `\n` 保留（多行文本由渲染组件按行拆分）；
//! - 其余控制字符（C0/C1/DEL，含 ESC 与 C1-CSI）替换为 `U+FFFD`，
//!   阻断 ANSI 显示注入并消除零宽吞字。

/// 把文本中的控制字符归一化为可安全显示的形态。
pub fn normalize_display_control_chars(text: &str) -> String {
    if !text.chars().any(char::is_control) {
        return text.to_string();
    }
    let mut normalized = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '\t' => normalized.push_str("    "),
            '\n' => normalized.push('\n'),
            other if other.is_control() => normalized.push('\u{fffd}'),
            other => normalized.push(other),
        }
    }
    normalized
}

#[cfg(test)]
mod tests {
    use super::normalize_display_control_chars;

    #[test]
    fn test_tab_expands_to_four_spaces() {
        assert_eq!(normalize_display_control_chars("a\tb"), "a    b");
    }

    #[test]
    fn test_multiple_tabs_expand_independently() {
        assert_eq!(
            normalize_display_control_chars("wanaka_session\t9892\t78"),
            "wanaka_session    9892    78"
        );
    }

    #[test]
    fn test_newline_preserved() {
        assert_eq!(normalize_display_control_chars("a\nb"), "a\nb");
    }

    #[test]
    fn test_escape_char_replaced_with_replacement_char() {
        assert_eq!(
            normalize_display_control_chars("a\u{1b}[31m"),
            "a\u{fffd}[31m"
        );
    }

    #[test]
    fn test_c1_csi_replaced() {
        assert_eq!(normalize_display_control_chars("a\u{9b}0m"), "a\u{fffd}0m");
    }

    #[test]
    fn test_bell_and_del_replaced() {
        assert_eq!(
            normalize_display_control_chars("a\u{7}b\u{7f}"),
            "a\u{fffd}b\u{fffd}"
        );
    }

    #[test]
    fn test_plain_text_unchanged() {
        assert_eq!(
            normalize_display_control_chars("你好 hello 2026"),
            "你好 hello 2026"
        );
    }

    #[test]
    fn test_zero_width_format_chars_preserved() {
        assert_eq!(normalize_display_control_chars("a\u{200b}b"), "a\u{200b}b");
    }
}
