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
#[path = "display_text_tests.rs"]
mod tests;
