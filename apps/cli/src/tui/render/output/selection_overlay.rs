//! 选区高亮唯一上色路径：只设 bg，保留原 fg，按字符边界 split span。

use crate::tui::render::output::rendered::RenderedLine;
use crate::tui::render::theme;
use ratatui::style::Style;
use ratatui::text::Span;

/// 单行内的选区范围（基于该行 plain 的字符偏移，半开区间 [start, end)）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SelRange {
    pub start: usize,
    pub end: usize,
}

pub fn apply_selection_overlay(
    line: &RenderedLine,
    selection: Option<SelRange>,
) -> Vec<Span<'static>> {
    let Some(SelRange { start, end }) = selection else {
        return line.spans.clone();
    };
    if start >= end {
        return line.spans.clone();
    }

    let mut out = Vec::new();
    let mut global = 0usize;
    for span in &line.spans {
        let mut buf = String::new();
        let mut current_selected: Option<bool> = None;
        for ch in span.content.chars() {
            let selected = global >= start && global < end;
            if current_selected != Some(selected) {
                if !buf.is_empty() {
                    out.push(make_span(
                        std::mem::take(&mut buf),
                        span.style,
                        current_selected.unwrap_or(false),
                    ));
                }
                current_selected = Some(selected);
            }
            buf.push(ch);
            global += 1;
        }
        if !buf.is_empty() {
            out.push(make_span(
                buf,
                span.style,
                current_selected.unwrap_or(false),
            ));
        }
    }
    out
}

fn make_span(text: String, base: Style, selected: bool) -> Span<'static> {
    let style = if selected {
        base.bg(theme::SELECTION_BG)
    } else {
        base
    };
    Span::styled(text, style)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Color;

    fn line() -> RenderedLine {
        RenderedLine::new(vec![Span::styled("hello", Style::default().fg(Color::Red))])
    }

    #[test]
    fn test_overlay_none_returns_original_spans() {
        let spans = apply_selection_overlay(&line(), None);

        assert_eq!(spans[0].style.fg, Some(Color::Red));
        assert!(spans.iter().all(|span| span.style.bg.is_none()));
    }

    #[test]
    fn test_overlay_sets_bg_keeps_fg() {
        let spans = apply_selection_overlay(&line(), Some(SelRange { start: 1, end: 4 }));
        let visible = spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();
        let selected = spans
            .iter()
            .filter(|span| span.style.bg == Some(theme::SELECTION_BG))
            .collect::<Vec<_>>();

        assert_eq!(visible, "hello");
        assert!(!selected.is_empty());
        assert!(
            selected
                .iter()
                .all(|span| span.style.fg == Some(Color::Red)),
            "保留原前景色（修 #61）"
        );
    }

    #[test]
    fn test_overlay_cjk_offset_by_char_not_byte() {
        let line = RenderedLine::new(vec![Span::raw("你好世界")]);
        let spans = apply_selection_overlay(&line, Some(SelRange { start: 1, end: 3 }));
        let selected = spans
            .iter()
            .filter(|span| span.style.bg.is_some())
            .map(|span| span.content.as_ref())
            .collect::<String>();

        assert_eq!(selected, "好世", "按字符而非字节偏移（修 #48/#51）");
    }
}
