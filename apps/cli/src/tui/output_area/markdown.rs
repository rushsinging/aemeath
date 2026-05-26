use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthChar;

use crate::tui::display::theme;

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
            '_' if is_flanking(result.chars().last(), chars.peek().copied()) => {
                strip_delimited(&mut result, &mut chars, "_", "_");
            }
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

/// 将 Markdown 文本解析为 styled Span 后再按显示宽度切分成屏幕行。
///
/// 必须先解析 Markdown 再换行，避免行内代码标记（如反引号）恰好跨越
/// wrap 边界时被当成未闭合标记，导致背景截断或文字溢出。
pub fn inline_markdown_lines(
    text: &str,
    base_style: Style,
    max_width: usize,
) -> Vec<Line<'static>> {
    wrap_spans(inline_markdown_spans(text, base_style), max_width)
}

fn wrap_spans(spans: Vec<Span<'static>>, max_width: usize) -> Vec<Line<'static>> {
    if max_width == 0 {
        return vec![Line::from(spans)];
    }

    let mut lines: Vec<Vec<Span<'static>>> = Vec::new();
    let mut current: Vec<Span<'static>> = Vec::new();
    let mut current_text = String::new();
    let mut current_style: Option<Style> = None;
    let mut current_width = 0usize;

    for span in spans {
        for ch in span.content.chars() {
            let ch_width = ch.width().unwrap_or(1);
            if !current_text.is_empty() && current_width + ch_width > max_width {
                flush_span(&mut current, &mut current_text, &mut current_style);
                lines.push(std::mem::take(&mut current));
                current_width = 0;
            }
            if current_style != Some(span.style) {
                flush_span(&mut current, &mut current_text, &mut current_style);
                current_style = Some(span.style);
            }
            current_text.push(ch);
            current_width += ch_width;
        }
    }

    flush_span(&mut current, &mut current_text, &mut current_style);
    if !current.is_empty() || lines.is_empty() {
        lines.push(current);
    }

    lines.into_iter().map(Line::from).collect()
}

fn flush_span(spans: &mut Vec<Span<'static>>, text: &mut String, style: &mut Option<Style>) {
    if !text.is_empty() {
        spans.push(Span::styled(
            std::mem::take(text),
            style.unwrap_or_default(),
        ));
    }
}

/// 将纯文本中的内联 Markdown 标记转换为 styled Span 列表。
///
/// 支持的语法：
/// - `**bold**`, `__bold__` → Bold
/// - `*italic*`, `_italic_` → Italic
/// - `` `code` `` → Code (强调色)
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
            '_' if is_flanking(buf.chars().last(), chars.peek().copied()) => push_delimited(
                &mut spans,
                &mut buf,
                &mut chars,
                "_",
                base_style.add_modifier(Modifier::ITALIC),
                base_style,
                "_",
            ),
            '_' => buf.push('_'),
            '`' => push_delimited(
                &mut spans,
                &mut buf,
                &mut chars,
                "`",
                base_style.fg(theme::CODE),
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
            .fg(theme::LINK)
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

/// 判断 `_` 是否处于 flanking 位置（可作为斜体标记）。
/// 左侧 flanking：前面是 None(行首)/空白/标点，后面是字母/数字。
/// 右侧 flanking：后面是 None(行尾)/空白/标点。
/// `_` 只在左 flanking 时才作为开标记触发。
fn is_flanking(prev: Option<char>, next: Option<char>) -> bool {
    let left_ok = prev.map_or(true, |c| !c.is_alphanumeric());
    let right_ok = next.map_or(false, |c| !c.is_whitespace() && !is_punctuation(c));
    left_ok && right_ok
}

fn is_punctuation(ch: char) -> bool {
    matches!(
        ch,
        '!' | '"'
            | '#'
            | '$'
            | '%'
            | '&'
            | '\''
            | '('
            | ')'
            | '*'
            | '+'
            | ','
            | '-'
            | '.'
            | '/'
            | ':'
            | ';'
            | '<'
            | '='
            | '>'
            | '?'
            | '@'
            | '['
            | '\\'
            | ']'
            | '^'
            | '_'
            | '`'
            | '{'
            | '|'
            | '}'
            | '~'
    )
}

/// 将累积的纯文本刷出为一个 Span
fn flush_plain(spans: &mut Vec<Span<'static>>, buf: &mut String, base_style: Style) {
    if !buf.is_empty() {
        spans.push(Span::styled(std::mem::take(buf), base_style));
    }
}
