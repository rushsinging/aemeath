use ratatui::{
    style::{Color, Style},
    text::Span,
};

use aemeath_core::string_idx::CharIdx;

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

    /// 是否有实际选中范围（start != end）
    pub(super) fn has_real_selection(&self) -> bool {
        match (self.selection_start, self.selection_end) {
            (Some((ss, sc)), Some((es, ec))) => ss != es || sc != ec,
            _ => false,
        }
    }

    /// 对已有的 markdown spans 叠加选中高亮。
    pub(super) fn render_spans_with_selection(
        &self,
        screen_idx: usize,
        spans: &[Span<'static>],
        screen_map: &[(usize, CharIdx, CharIdx)],
    ) -> Vec<Span<'static>> {
        let Some((start_logic, start_col)) = self.selection_start else {
            return spans.to_vec();
        };
        let Some((end_logic, end_col)) = self.selection_end else {
            return spans.to_vec();
        };

        let (start_logic, start_col, end_logic, end_col) =
            if start_logic < end_logic || (start_logic == end_logic && start_col < end_col) {
                (start_logic, start_col, end_logic, end_col)
            } else {
                (end_logic, end_col, start_logic, start_col)
            };

        if start_logic == end_logic && start_col == end_col {
            return spans.to_vec();
        }

        let current_logic = if screen_idx < screen_map.len() {
            screen_map[screen_idx].0
        } else {
            return spans.to_vec();
        };
        if current_logic < start_logic || current_logic > end_logic {
            return spans.to_vec();
        }

        let all_chars: Vec<(char, Style)> = spans
            .iter()
            .flat_map(|span| span.content.chars().map(|ch| (ch, span.style)))
            .collect();
        let chunk_start = screen_map[screen_idx].1;
        let line_start = if current_logic == start_logic {
            start_col.saturating_sub(chunk_start)
        } else {
            0
        };
        let line_end = if current_logic == end_logic {
            end_col.saturating_sub(chunk_start).min(all_chars.len())
        } else {
            all_chars.len()
        };

        render_selected_chars(all_chars.into_iter(), line_start, line_end)
    }
}

fn render_selected_chars(
    chars: impl Iterator<Item = (char, Style)>,
    line_start: usize,
    line_end: usize,
) -> Vec<Span<'static>> {
    let selection_style = Style::default().bg(Color::Blue).fg(Color::White);
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
