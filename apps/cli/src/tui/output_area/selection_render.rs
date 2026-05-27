use ratatui::{style::Style, text::Line, text::Span};

use sdk::CharIdx;

use crate::tui::display::theme;

impl super::OutputArea {
    /// 渲染带选择高亮的单行（screen_idx 是屏幕行索引）
    pub(super) fn render_line_with_selection(
        &self,
        screen_idx: usize,
        content: &str,
        base_style: Style,
        screen_map: &[(usize, CharIdx, CharIdx)],
    ) -> Vec<Span<'static>> {
        let Some((start_logic, start_col)) = self.selection_start else {
            return vec![Span::styled(content.to_string(), base_style)];
        };
        let Some((end_logic, end_col)) = self.selection_end else {
            return vec![Span::styled(content.to_string(), base_style)];
        };

        let (start_logic, start_col, end_logic, end_col) =
            if start_logic < end_logic || (start_logic == end_logic && start_col < end_col) {
                (start_logic, start_col, end_logic, end_col)
            } else {
                (end_logic, end_col, start_logic, start_col)
            };

        let current_logic = if screen_idx < screen_map.len() {
            screen_map[screen_idx].0
        } else {
            return vec![Span::styled(content.to_string(), base_style)];
        };

        if current_logic < start_logic || current_logic > end_logic {
            return vec![Span::styled(content.to_string(), base_style)];
        }
        if start_logic == end_logic && start_col == end_col {
            return vec![Span::styled(content.to_string(), base_style)];
        }

        let chars: Vec<char> = content.chars().collect();
        let chunk_start = screen_map[screen_idx].1;
        let line_start = if current_logic == start_logic {
            start_col.saturating_sub(chunk_start)
        } else {
            0
        };
        let line_end = if current_logic == end_logic {
            end_col.saturating_sub(chunk_start).min(chars.len())
        } else {
            chars.len()
        };

        render_selected_chars(
            chars.into_iter().map(|ch| (ch, base_style)),
            line_start,
            line_end,
        )
    }

    /// 对已渲染的 styled line 叠加选区高亮。
    /// `screen_idx` 是屏幕行索引，`screen_map` 提供逻辑行和 char 偏移信息。
    pub(super) fn apply_selection_to_line(
        &self,
        screen_idx: usize,
        line: &Line<'static>,
        screen_map: &[(usize, CharIdx, CharIdx)],
    ) -> Line<'static> {
        let Some((start_logic, start_col)) = self.selection_start else {
            return line.clone();
        };
        let Some((end_logic, end_col)) = self.selection_end else {
            return line.clone();
        };

        let (start_logic, start_col, end_logic, end_col) =
            if start_logic < end_logic || (start_logic == end_logic && start_col < end_col) {
                (start_logic, start_col, end_logic, end_col)
            } else {
                (end_logic, end_col, start_logic, start_col)
            };

        if screen_idx >= screen_map.len() {
            return line.clone();
        }
        let current_logic = screen_map[screen_idx].0;

        if current_logic < start_logic || current_logic > end_logic {
            return line.clone();
        }
        if start_logic == end_logic && start_col == end_col {
            return line.clone();
        }

        let chunk_start = screen_map[screen_idx].1;
        let sel_start = if current_logic == start_logic {
            start_col.saturating_sub(chunk_start)
        } else {
            0
        };
        let sel_end = if current_logic == end_logic {
            end_col.saturating_sub(chunk_start)
        } else {
            usize::MAX
        };

        let mut new_spans = Vec::new();
        let mut global_offset = 0usize;

        for span in line.spans.iter() {
            let span_char_count = span.content.chars().count();
            let span_start = global_offset;
            let span_end = span_start + span_char_count;

            if span_end <= sel_start || span_start >= sel_end || sel_start == sel_end {
                new_spans.push(span.clone());
                global_offset += span_char_count;
                continue;
            }

            // 需要拆分这个 span
            let mut buf = String::new();
            let mut current_style: Option<Style> = None;

            for (i, ch) in span.content.chars().enumerate() {
                let char_global = span_start + i;
                let is_selected = char_global >= sel_start && char_global < sel_end;
                let style = if is_selected {
                    let mut s = span.style;
                    s.fg = Some(theme::SELECTION_FG);
                    s.bg = Some(theme::SELECTION_BG);
                    s
                } else {
                    span.style
                };

                if current_style != Some(style) {
                    if !buf.is_empty() {
                        new_spans.push(Span::styled(
                            std::mem::take(&mut buf),
                            current_style.unwrap_or(span.style),
                        ));
                    }
                    current_style = Some(style);
                }
                buf.push(ch);
            }

            if !buf.is_empty() {
                new_spans.push(Span::styled(buf, current_style.unwrap_or(span.style)));
            }

            global_offset += span_char_count;
        }

        Line::from(new_spans)
    }

    /// 是否有实际选中范围（start != end）
    pub(super) fn has_real_selection(&self) -> bool {
        match (self.selection_start, self.selection_end) {
            (Some((ss, sc)), Some((es, ec))) => ss != es || sc != ec,
            _ => false,
        }
    }
}

fn render_selected_chars(
    chars: impl Iterator<Item = (char, Style)>,
    line_start: usize,
    line_end: usize,
) -> Vec<Span<'static>> {
    let selection_style = Style::default()
        .bg(theme::SELECTION_BG)
        .fg(theme::SELECTION_FG);
    let mut result = Vec::new();
    let mut current_text = String::new();
    let mut current_style: Option<Style> = None;

    for (i, (ch, base_style)) in chars.enumerate() {
        let style = if i >= line_start && i < line_end {
            selection_style
        } else {
            base_style
        };
        if current_style != Some(style) {
            if !current_text.is_empty() {
                result.push(Span::styled(
                    std::mem::take(&mut current_text),
                    current_style.unwrap_or(base_style),
                ));
            }
            current_style = Some(style);
        }
        current_text.push(ch);
    }

    if !current_text.is_empty() {
        result.push(Span::styled(current_text, current_style.unwrap()));
    }
    result
}
