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
    // `global` 是 plain 字符坐标（与 SelRange 同坐标系）。前导 gutter 不进 plain，
    // 故必须跳过：gutter 字符原样输出且永不高亮，也不推进 `global`。
    let mut global = 0usize;
    let mut skipped = 0usize;
    let gutter_cols = line.gutter_cols;
    for span in &line.spans {
        let mut buf = String::new();
        let mut current_selected: Option<bool> = None;
        for ch in span.content.chars() {
            if skipped < gutter_cols {
                // 处于 gutter 区间：原样输出，不高亮，不推进 plain 坐标。
                skipped += 1;
                if current_selected != Some(false) {
                    if !buf.is_empty() {
                        out.push(make_span(
                            std::mem::take(&mut buf),
                            span.style,
                            current_selected.unwrap_or(false),
                        ));
                    }
                    current_selected = Some(false);
                }
                buf.push(ch);
                continue;
            }
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

pub fn apply_selection_overlay_with_fg(
    line: &RenderedLine,
    selection: Option<SelRange>,
    selected_fg: ratatui::style::Color,
) -> Vec<Span<'static>> {
    apply_selection_overlay(line, selection)
        .into_iter()
        .map(|span| {
            if span.style.bg == Some(theme::SELECTION_BG) {
                Span::styled(span.content.into_owned(), span.style.fg(selected_fg))
            } else {
                span
            }
        })
        .collect()
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

    /// 构造带 gutter 的行：首 span 为 gutter（不进 plain），其余为内容。
    fn gutter_line(gutter: &str, content: &str) -> RenderedLine {
        let mut line = RenderedLine::with_plain(
            vec![
                Span::raw(gutter.to_string()),
                Span::styled(content.to_string(), Style::default().fg(Color::Red)),
            ],
            content.to_string(),
        );
        line.gutter_cols = gutter.chars().count();
        line
    }

    #[test]
    fn test_overlay_skips_gutter_highlights_content_only() {
        // gutter_cols=2，spans=["✓ ", "hello"]，plain="hello"
        let line = gutter_line("✓ ", "hello");
        let spans = apply_selection_overlay(&line, Some(SelRange { start: 0, end: 2 }));

        let visible = spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();
        assert_eq!(visible, "✓ hello", "可见文本不变（gutter + 内容）");

        let selected = spans
            .iter()
            .filter(|span| span.style.bg == Some(theme::SELECTION_BG))
            .map(|span| span.content.as_ref())
            .collect::<String>();
        assert_eq!(selected, "he", "高亮内容首两字符，而非 gutter 或右移");
    }

    #[test]
    fn test_overlay_gutter_span_never_highlighted() {
        // 即便选区从 0 起，gutter 也绝不上 bg。
        let line = gutter_line("✓ ", "hello");
        let spans = apply_selection_overlay(&line, Some(SelRange { start: 0, end: 5 }));

        let gutter_highlighted = spans
            .iter()
            .filter(|span| span.style.bg == Some(theme::SELECTION_BG))
            .any(|span| span.content.as_ref().contains('✓') || span.content.as_ref() == " ");
        assert!(!gutter_highlighted, "gutter 是 chrome，永不高亮");
    }

    #[test]
    fn test_overlay_skips_gutter_with_cjk_content() {
        // gutter_cols=2，plain="你好世界"，SelRange{1,3} 高亮 "好世"
        let line = gutter_line("✓ ", "你好世界");
        let spans = apply_selection_overlay(&line, Some(SelRange { start: 1, end: 3 }));

        let selected = spans
            .iter()
            .filter(|span| span.style.bg == Some(theme::SELECTION_BG))
            .map(|span| span.content.as_ref())
            .collect::<String>();
        assert_eq!(selected, "好世", "CJK 内容按字符偏移，gutter 不高亮");
    }

    #[test]
    fn test_overlay_gutter_cols_zero_keeps_old_behavior() {
        // gutter_cols=0（默认）：行为与无 gutter 一致。
        let line = RenderedLine::new(vec![Span::styled("hello", Style::default().fg(Color::Red))]);
        assert_eq!(line.gutter_cols, 0);
        let spans = apply_selection_overlay(&line, Some(SelRange { start: 1, end: 4 }));

        let selected = spans
            .iter()
            .filter(|span| span.style.bg == Some(theme::SELECTION_BG))
            .map(|span| span.content.as_ref())
            .collect::<String>();
        assert_eq!(selected, "ell");
    }
}
