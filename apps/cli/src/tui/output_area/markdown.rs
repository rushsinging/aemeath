use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;

mod table;
#[cfg(test)]
mod tests;

pub use table::{is_table_row, is_table_separator, render_table_block};

/// 剥离内联 Markdown 格式标记，返回纯文本。
/// 用于复制选中内容时去除 `**`、`#`、`` ` `` 等格式标记。
///
/// 支持的语法：
/// - `**bold**`, `__bold__` → `bold`
/// - `*italic*`, `_italic_` → `italic`
/// - `` `code` `` → `code`
/// - `~~strikethrough~~` → `strikethrough`
/// - `[text](url)` → `text`
/// - 格式不完整时保留原始文本
pub fn strip_inline_formatting(text: &str) -> String {
    let mut result = String::new();
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            '*' if chars.peek() == Some(&'*') => {
                chars.next();
                strip_delimited(&mut result, &mut chars, "**", "**");
            }
            '_' if chars.peek() == Some(&'_') => {
                chars.next();
                strip_delimited(&mut result, &mut chars, "__", "__");
            }
            '*' => strip_delimited(&mut result, &mut chars, "*", "*"),
            '_' => strip_delimited(&mut result, &mut chars, "_", "_"),
            '`' => strip_delimited(&mut result, &mut chars, "`", "`"),
            '~' if chars.peek() == Some(&'~') => {
                chars.next();
                strip_delimited(&mut result, &mut chars, "~~", "~~");
            }
            '[' => {
                if !strip_link(&mut result, &mut chars) {
                    result.push('[');
                }
            }
            other => result.push(other),
        }
    }
    result
}

fn strip_delimited(
    result: &mut String,
    chars: &mut std::iter::Peekable<std::str::Chars>,
    marker: &str,
    literal: &str,
) {
    match find_closing(chars, marker) {
        Some(inner) => {
            result.push_str(&inner);
            advance_chars(chars, inner.chars().count() + marker.chars().count());
        }
        None => result.push_str(literal),
    }
}

fn strip_link(result: &mut String, chars: &mut std::iter::Peekable<std::str::Chars>) -> bool {
    let rest: String = chars.clone().collect();
    let Some(close_bracket) = rest.find(']') else {
        return false;
    };
    let after_bracket = rest.get(close_bracket + 1..).unwrap_or("");
    if !after_bracket.starts_with('(') {
        return false;
    }
    let url_start = close_bracket + 2;
    let url_rest = rest.get(url_start..).unwrap_or("");
    let Some(close_paren) = url_rest.find(')') else {
        return false;
    };
    let inner = rest.get(0..close_bracket).unwrap_or("");
    let url = rest.get(url_start..url_start + close_paren).unwrap_or("");
    if inner.is_empty() || url.is_empty() {
        return false;
    }

    result.push_str(inner);
    advance_chars(
        chars,
        inner.chars().count() + 1 + 1 + url.chars().count() + 1,
    );
    true
}

/// 将纯文本中的内联 Markdown 标记转换为 styled Span 列表。
///
/// 支持的语法：
/// - `**bold**`, `__bold__` → Bold
/// - `*italic*`, `_italic_` → Italic
/// - `` `code` `` → Code (深色背景)
/// - `~~strikethrough~~` → Strikethrough
/// - `[text](url)` → 链接（青色+下划线样式）
/// - 格式不完整时原样显示原始文本
pub fn inline_markdown_spans(text: &str, base_style: Style) -> Vec<Span<'static>> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut buf = String::new();
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            '*' if chars.peek() == Some(&'*') => {
                chars.next();
                push_delimited(
                    &mut spans,
                    &mut buf,
                    &mut chars,
                    "**",
                    base_style.add_modifier(Modifier::BOLD),
                    base_style,
                    "**",
                );
            }
            '_' if chars.peek() == Some(&'_') => {
                chars.next();
                push_delimited(
                    &mut spans,
                    &mut buf,
                    &mut chars,
                    "__",
                    base_style.add_modifier(Modifier::BOLD),
                    base_style,
                    "__",
                );
            }
            '*' => push_delimited(
                &mut spans,
                &mut buf,
                &mut chars,
                "*",
                base_style.add_modifier(Modifier::ITALIC),
                base_style,
                "*",
            ),
            '_' => push_delimited(
                &mut spans,
                &mut buf,
                &mut chars,
                "_",
                base_style.add_modifier(Modifier::ITALIC),
                base_style,
                "_",
            ),
            '`' => push_delimited(
                &mut spans,
                &mut buf,
                &mut chars,
                "`",
                base_style
                    .bg(Color::Rgb(40, 44, 52))
                    .fg(Color::Rgb(171, 178, 191)),
                base_style,
                "`",
            ),
            '~' if chars.peek() == Some(&'~') => {
                chars.next();
                push_delimited(
                    &mut spans,
                    &mut buf,
                    &mut chars,
                    "~~",
                    base_style.add_modifier(Modifier::CROSSED_OUT),
                    base_style,
                    "~~",
                );
            }
            '[' => {
                if !push_link(&mut spans, &mut buf, &mut chars, base_style) {
                    buf.push('[');
                }
            }
            other => buf.push(other),
        }
    }

    flush_plain(&mut spans, &mut buf, base_style);
    spans
}

fn push_delimited(
    spans: &mut Vec<Span<'static>>,
    buf: &mut String,
    chars: &mut std::iter::Peekable<std::str::Chars>,
    marker: &str,
    style: Style,
    base_style: Style,
    literal: &str,
) {
    match find_closing(chars, marker) {
        Some(inner) => {
            flush_plain(spans, buf, base_style);
            spans.push(Span::styled(inner.clone(), style));
            advance_chars(chars, inner.chars().count() + marker.chars().count());
        }
        None => buf.push_str(literal),
    }
}

fn push_link(
    spans: &mut Vec<Span<'static>>,
    buf: &mut String,
    chars: &mut std::iter::Peekable<std::str::Chars>,
    base_style: Style,
) -> bool {
    let rest: String = chars.clone().collect();
    let Some(close_bracket) = rest.find(']') else {
        return false;
    };
    let after_bracket = rest.get(close_bracket + 1..).unwrap_or("");
    if !after_bracket.starts_with('(') {
        return false;
    }
    let url_start = close_bracket + 2;
    let url_rest = rest.get(url_start..).unwrap_or("");
    let Some(close_paren) = url_rest.find(')') else {
        return false;
    };
    let inner = rest.get(0..close_bracket).unwrap_or("");
    let url = rest.get(url_start..url_start + close_paren).unwrap_or("");
    if inner.is_empty() || url.is_empty() {
        return false;
    }

    flush_plain(spans, buf, base_style);
    spans.push(Span::styled(
        inner.to_string(),
        base_style
            .fg(Color::Cyan)
            .add_modifier(Modifier::UNDERLINED),
    ));
    advance_chars(
        chars,
        inner.chars().count() + 1 + 1 + url.chars().count() + 1,
    );
    true
}

/// 在迭代器的剩余字符串中查找 closing 标记。如果找到，返回标记前的文本。
/// **不会**消耗迭代器（使用 clone 做前瞻）。
fn find_closing(chars: &std::iter::Peekable<std::str::Chars>, close: &str) -> Option<String> {
    let rest: String = chars.clone().collect();
    rest.find(close).map(|end| rest[..end].to_string())
}

/// 将迭代器前进 n 个字符
fn advance_chars(chars: &mut std::iter::Peekable<std::str::Chars>, n: usize) {
    for _ in 0..n {
        chars.next();
    }
}

/// 将累积的纯文本刷出为一个 Span
fn flush_plain(spans: &mut Vec<Span<'static>>, buf: &mut String, base_style: Style) {
    if !buf.is_empty() {
        spans.push(Span::styled(std::mem::take(buf), base_style));
    }
}
