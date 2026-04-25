use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;

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
            // **bold**
            '*' if chars.peek() == Some(&'*') => {
                chars.next(); // consume second '*'
                match find_closing(&chars, "**") {
                    Some(inner) => {
                        flush_plain(&mut spans, &mut buf, base_style);
                        spans.push(Span::styled(inner.clone(), base_style.add_modifier(Modifier::BOLD)));
                        advance_chars(&mut chars, inner.chars().count() + 2);
                    }
                    None => buf.push_str("**"),
                }
            }
            // __bold__
            '_' if chars.peek() == Some(&'_') => {
                chars.next(); // consume second '_'
                match find_closing(&chars, "__") {
                    Some(inner) => {
                        flush_plain(&mut spans, &mut buf, base_style);
                        spans.push(Span::styled(inner.clone(), base_style.add_modifier(Modifier::BOLD)));
                        advance_chars(&mut chars, inner.chars().count() + 2);
                    }
                    None => buf.push_str("__"),
                }
            }
            // *italic*
            '*' => match find_closing(&chars, "*") {
                Some(inner) => {
                    flush_plain(&mut spans, &mut buf, base_style);
                    spans.push(Span::styled(inner.clone(), base_style.add_modifier(Modifier::ITALIC)));
                    advance_chars(&mut chars, inner.chars().count() + 1);
                }
                None => buf.push('*'),
            },
            // _italic_
            '_' => match find_closing(&chars, "_") {
                Some(inner) => {
                    flush_plain(&mut spans, &mut buf, base_style);
                    spans.push(Span::styled(inner.clone(), base_style.add_modifier(Modifier::ITALIC)));
                    advance_chars(&mut chars, inner.chars().count() + 1);
                }
                None => buf.push('_'),
            },
            // `code`
            '`' => match find_closing(&chars, "`") {
                Some(inner) => {
                    flush_plain(&mut spans, &mut buf, base_style);
                    spans.push(Span::styled(
                        inner.clone(),
                        base_style.bg(Color::DarkGray).add_modifier(Modifier::DIM),
                    ));
                    advance_chars(&mut chars, inner.chars().count() + 1);
                }
                None => buf.push('`'),
            },
            // ~~strikethrough~~
            '~' if chars.peek() == Some(&'~') => {
                chars.next(); // consume second '~'
                match find_closing(&chars, "~~") {
                    Some(inner) => {
                        flush_plain(&mut spans, &mut buf, base_style);
                        spans.push(Span::styled(inner.clone(), base_style.add_modifier(Modifier::CROSSED_OUT)));
                        advance_chars(&mut chars, inner.chars().count() + 2);
                    }
                    None => buf.push_str("~~"),
                }
            }
            // [text](url)
            '[' => {
                let rest: String = chars.clone().collect();
                if let Some(close_bracket) = rest.find(']') {
                    if rest[close_bracket + 1..].starts_with('(') {
                        if let Some(close_paren) = rest[close_bracket + 2..].find(')') {
                            let inner = &rest[..close_bracket];
                            let _url = &rest[close_bracket + 2..close_bracket + 2 + close_paren];
                            if !inner.is_empty() && !_url.is_empty() {
                                flush_plain(&mut spans, &mut buf, base_style);
                                spans.push(Span::styled(
                                    inner.to_string(),
                                    base_style.fg(Color::Cyan).add_modifier(Modifier::UNDERLINED),
                                ));
                                // advance: inner + "]" + "(" + url + ")"
                                advance_chars(&mut chars, inner.chars().count() + 1 + 1 + _url.chars().count() + 1);
                                continue; // consumed through the ')', keep going
                            }
                        }
                    }
                }
                // fallback: output '[' literally
                buf.push('[');
            }
            other => {
                buf.push(other);
            }
        }
    }

    flush_plain(&mut spans, &mut buf, base_style);
    spans
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

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Style {
        Style::default()
    }

    #[test]
    fn plain_text() {
        let spans = inline_markdown_spans("hello world", base());
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].content, "hello world");
    }

    #[test]
    fn bold_text() {
        let spans = inline_markdown_spans("**bold**", base());
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].content, "bold");
        assert!(spans[0].style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn italic_text() {
        let spans = inline_markdown_spans("*italic*", base());
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].content, "italic");
    }

    #[test]
    fn inline_code() {
        let spans = inline_markdown_spans("use `HashMap` here", base());
        assert_eq!(spans.len(), 3);
        assert_eq!(spans[0].content, "use ");
        assert_eq!(spans[1].content, "HashMap");
        assert_eq!(spans[2].content, " here");
    }

    #[test]
    fn mixed_formatting() {
        let spans = inline_markdown_spans("**bold** and *italic*", base());
        assert_eq!(spans.len(), 3);
        assert_eq!(spans[0].content, "bold");
        assert_eq!(spans[1].content, " and ");
        assert_eq!(spans[2].content, "italic");
    }

    #[test]
    fn strikethrough() {
        let spans = inline_markdown_spans("~~deleted~~", base());
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].content, "deleted");
    }

    #[test]
    fn unclosed_marker_outputs_literal() {
        let spans = inline_markdown_spans("**unclosed", base());
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].content, "**unclosed");
    }

    #[test]
    fn link() {
        let spans = inline_markdown_spans("click [here](https://example.com) now", base());
        assert_eq!(spans.len(), 3);
        assert_eq!(spans[0].content, "click ");
        assert_eq!(spans[1].content, "here");
        assert_eq!(spans[2].content, " now");
    }

    #[test]
    fn empty_string() {
        let spans = inline_markdown_spans("", base());
        assert_eq!(spans.len(), 0);
    }

    #[test]
    fn multiple_bold_spans() {
        let spans = inline_markdown_spans("**a** and **b**", base());
        assert_eq!(spans.len(), 3);
        assert_eq!(spans[0].content, "a");
        assert_eq!(spans[1].content, " and ");
        assert_eq!(spans[2].content, "b");
    }
}
