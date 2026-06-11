use crate::tui::render::output::rendered::RenderedLine;
use ratatui::text::Span;
use unicode_width::UnicodeWidthChar;

pub fn wrap_spans_to_rendered_lines(
    spans: Vec<Span<'static>>,
    max_width: usize,
) -> Vec<RenderedLine> {
    wrap_spans_with_prefix(spans, max_width, None)
}

pub fn wrap_spans_with_prefix(
    spans: Vec<Span<'static>>,
    max_width: usize,
    continuation_prefix: Option<Span<'static>>,
) -> Vec<RenderedLine> {
    if max_width == 0 {
        return vec![RenderedLine::new(spans)];
    }

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
                if let Some(prefix) = continuation_prefix.as_ref() {
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
        let lines = wrap_spans_with_prefix(vec![Span::raw("> abcdef")], 6, Some(Span::raw("  ")));

        assert_eq!(
            lines
                .iter()
                .map(|line| line.plain.as_str())
                .collect::<Vec<_>>(),
            vec!["> abcd", "  ef"]
        );
    }
}
