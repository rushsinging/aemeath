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
#[path = "selection_overlay_tests.rs"]
mod tests;
