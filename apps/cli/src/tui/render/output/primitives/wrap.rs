use crate::tui::render::output::rendered::RenderedLine;
use ratatui::text::Span;
use unicode_width::UnicodeWidthChar;

/// 断行模式。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WrapMode {
    /// 逐字符断行（代码块、状态行等不应破坏词边界的场景）。
    Char,
    /// 词边界优先断行，超长词回退字符断（正文、选项描述等可读性优先场景）。
    Word,
}

pub fn wrap_spans_to_rendered_lines(
    spans: Vec<Span<'static>>,
    max_width: usize,
) -> Vec<RenderedLine> {
    wrap_spans_with_prefix(spans, max_width, None, WrapMode::Char)
}

pub fn wrap_spans_with_prefix(
    spans: Vec<Span<'static>>,
    max_width: usize,
    continuation_prefix: Option<Span<'static>>,
    mode: WrapMode,
) -> Vec<RenderedLine> {
    if max_width == 0 {
        return vec![RenderedLine::new(spans)];
    }

    match mode {
        WrapMode::Char => wrap_char(&spans, max_width, continuation_prefix.as_ref()),
        WrapMode::Word => wrap_word(&spans, max_width, continuation_prefix.as_ref()),
    }
}

/// Char 模式：逐字符断行（保持原行为）。
fn wrap_char(
    spans: &[Span<'static>],
    max_width: usize,
    continuation_prefix: Option<&Span<'static>>,
) -> Vec<RenderedLine> {
    let mut out: Vec<RenderedLine> = Vec::new();
    let mut current: Vec<Span<'static>> = Vec::new();
    let mut current_text = String::new();
    let mut current_style = None;
    let mut current_width = 0usize;
    let mut line_started = false;

    for span in spans {
        for ch in span.content.chars() {
            let ch_width = ch.width().unwrap_or(0);
            if line_started && current_width + ch_width > max_width {
                flush_span(&mut current, &mut current_text, &mut current_style);
                out.push(RenderedLine::new(std::mem::take(&mut current)));
                current_width = 0;
                if let Some(prefix) = continuation_prefix {
                    push_span_text(&mut current, &mut current_width, prefix.clone());
                }
            }
            if current_style != Some(span.style) {
                flush_span(&mut current, &mut current_text, &mut current_style);
                current_style = Some(span.style);
            }
            current_text.push(ch);
            current_width += ch_width;
            line_started = true;
        }
    }

    flush_span(&mut current, &mut current_text, &mut current_style);
    if !current.is_empty() || out.is_empty() {
        out.push(RenderedLine::new(current));
    }
    out
}

/// Word 模式：优先在空格处断行，超长词回退字符断。
fn wrap_word(
    spans: &[Span<'static>],
    max_width: usize,
    continuation_prefix: Option<&Span<'static>>,
) -> Vec<RenderedLine> {
    // 展平为 (char, style)
    let chars: Vec<(char, ratatui::style::Style)> = spans
        .iter()
        .flat_map(|span| span.content.chars().map(move |c| (c, span.style)))
        .collect();

    let prefix_width = continuation_prefix
        .map(|p| p.content.chars().filter_map(|c| c.width()).sum::<usize>())
        .unwrap_or(0);

    let mut out: Vec<RenderedLine> = Vec::new();
    let mut line_chars: Vec<(char, ratatui::style::Style)> = Vec::new();
    let mut line_width = prefix_width;

    // 行首补 prefix
    if let Some(p) = continuation_prefix {
        for c in p.content.chars() {
            line_chars.push((c, p.style));
        }
    }

    let mut new_line = |line_chars: &mut Vec<(char, ratatui::style::Style)>,
                        line_width: &mut usize| {
        push_line(line_chars, &mut out);
        line_chars.clear();
        *line_width = prefix_width;
        if let Some(p) = continuation_prefix {
            for c in p.content.chars() {
                line_chars.push((c, p.style));
            }
        }
    };

    let mut iter = chars.into_iter().peekable();
    while let Some((ch, style)) = iter.next() {
        let ch_width = ch.width().unwrap_or(0);

        if ch == ' ' {
            // 空格：尝试放入当前行尾
            if line_width + ch_width > max_width {
                new_line(&mut line_chars, &mut line_width);
                // 行首空格丢弃
                continue;
            }
            line_chars.push((ch, style));
            line_width += ch_width;
            continue;
        }

        // 非空格：贪婪累积一个完整词（直到空格或结尾）
        let mut word: Vec<(char, ratatui::style::Style)> = vec![(ch, style)];
        let mut word_w = ch_width;
        while let Some(&(nc, _)) = iter.peek() {
            if nc == ' ' {
                break;
            }
            let (nc, ns) = iter.next().unwrap();
            word_w += nc.width().unwrap_or(0);
            word.push((nc, ns));
        }

        // 尝试把整个词放入当前行
        if line_width + word_w <= max_width {
            line_chars.extend(word);
            line_width += word_w;
        } else if word_w <= max_width.saturating_sub(prefix_width) {
            // 词放不下当前行但能放下一行：断行
            new_line(&mut line_chars, &mut line_width);
            line_chars.extend(word);
            line_width += word_w;
        } else {
            // 词本身超过可用宽度：字符断
            for (wc, ws) in word {
                let ww = wc.width().unwrap_or(0);
                if line_width + ww > max_width && line_width > prefix_width {
                    new_line(&mut line_chars, &mut line_width);
                }
                line_chars.push((wc, ws));
                line_width += ww;
            }
        }
    }

    push_line(&mut line_chars, &mut out);
    out
}

fn push_line(chars: &mut Vec<(char, ratatui::style::Style)>, out: &mut Vec<RenderedLine>) {
    let spans = collapse_to_spans(std::mem::take(chars));
    out.push(RenderedLine::new(spans));
}

/// 把 (char, style) 序列合并为连续相同样式的 spans。
fn collapse_to_spans(chars: Vec<(char, ratatui::style::Style)>) -> Vec<Span<'static>> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut buf = String::new();
    let mut cur_style: Option<ratatui::style::Style> = None;
    for (ch, style) in chars {
        if cur_style != Some(style) {
            if !buf.is_empty() {
                spans.push(Span::styled(
                    std::mem::take(&mut buf),
                    cur_style.unwrap_or_default(),
                ));
            }
            cur_style = Some(style);
        }
        buf.push(ch);
    }
    if !buf.is_empty() {
        spans.push(Span::styled(buf, cur_style.unwrap_or_default()));
    }
    spans
}

/// 将纯文本按指定显示宽度断行为多行字符串（String 级入口，供 display 等使用）。
pub fn wrap_text_to_strings(text: &str, max_width: usize, mode: WrapMode) -> Vec<String> {
    let lines = wrap_spans_with_prefix(vec![Span::raw(text.to_string())], max_width, None, mode);
    lines.into_iter().map(|l| l.plain).collect()
}

fn push_span_text(
    current: &mut Vec<Span<'static>>,
    current_width: &mut usize,
    span: Span<'static>,
) {
    *current_width += span
        .content
        .chars()
        .filter_map(|ch| ch.width())
        .sum::<usize>();
    current.push(span);
}

fn flush_span(
    spans: &mut Vec<Span<'static>>,
    text: &mut String,
    style: &mut Option<ratatui::style::Style>,
) {
    if text.is_empty() {
        return;
    }
    spans.push(Span::styled(
        std::mem::take(text),
        style.unwrap_or_default(),
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::{Color, Style};

    #[test]
    fn test_wrap_spans_to_rendered_lines_splits_ascii_by_width() {
        let lines = wrap_spans_to_rendered_lines(vec![Span::raw("abcdef")], 4);

        assert_eq!(
            lines
                .iter()
                .map(|line| line.plain.as_str())
                .collect::<Vec<_>>(),
            vec!["abcd", "ef"]
        );
    }

    #[test]
    fn test_wrap_spans_to_rendered_lines_preserves_style_across_wrap() {
        let style = Style::default().fg(Color::Red);
        let lines = wrap_spans_to_rendered_lines(vec![Span::styled("abcdef", style)], 4);

        assert_eq!(lines[0].spans[0].style.fg, Some(Color::Red));
        assert_eq!(lines[1].spans[0].style.fg, Some(Color::Red));
    }

    #[test]
    fn test_wrap_spans_to_rendered_lines_handles_cjk_display_width() {
        let lines = wrap_spans_to_rendered_lines(vec![Span::raw("你好ab")], 4);

        assert_eq!(
            lines
                .iter()
                .map(|line| line.plain.as_str())
                .collect::<Vec<_>>(),
            vec!["你好", "ab"]
        );
    }

    #[test]
    fn test_wrap_spans_with_prefix_indents_continuation_lines() {
        let lines = wrap_spans_with_prefix(
            vec![Span::raw("> abcdef")],
            6,
            Some(Span::raw("  ")),
            WrapMode::Char,
        );

        assert_eq!(
            lines
                .iter()
                .map(|line| line.plain.as_str())
                .collect::<Vec<_>>(),
            vec!["> abcd", "  ef"]
        );
    }

    #[test]
    fn test_wrap_text_to_strings_word_mode_breaks_at_word_boundary() {
        // Word 模式：优先在空格处断行，不拆词
        let lines = wrap_text_to_strings("aaa bbb ccc ddd", 7, WrapMode::Word);
        assert_eq!(lines, vec!["aaa bbb", "ccc ddd"], "Word 模式应在词边界断行");
    }

    #[test]
    fn test_wrap_text_to_strings_word_mode_falls_back_to_char_for_overlong_word() {
        // 单个超长词仍需字符断，否则溢出
        let lines = wrap_text_to_strings("aaaaaaaaaa", 4, WrapMode::Word);
        assert_eq!(lines, vec!["aaaa", "aaaa", "aa"], "超长词应字符回退断行");
    }

    #[test]
    fn test_wrap_text_to_strings_word_mode_cjk_equivalent() {
        // CJK 无词边界，Word 与 Char 等价（逐字符按宽度断）
        let lines = wrap_text_to_strings("你好世界你好", 4, WrapMode::Word);
        assert_eq!(lines, vec!["你好", "世界", "你好"]);
    }

    #[test]
    fn test_wrap_text_to_strings_char_mode_keeps_current_behavior() {
        // Char 模式：逐字符硬切，保持现状
        let lines = wrap_text_to_strings("aaa bbb", 4, WrapMode::Char);
        assert_eq!(lines, vec!["aaa ", "bbb"]);
    }
}
