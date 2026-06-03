use super::InputArea;
use crate::tui::model::input::completion::{InputCompletion, Suggestion, SuggestionType};
use crate::tui::render::display::safe_text::truncate_unicode_width;
use crate::tui::render::theme;
use ratatui::{buffer::Buffer, layout::Rect, style::Style};
use unicode_width::UnicodeWidthChar;

pub struct SuggestionViewState {
    pub suggestions: Vec<Suggestion>,
    pub selected: Option<usize>,
}

impl SuggestionViewState {
    pub fn from_completion(completion: &InputCompletion) -> Self {
        Self {
            suggestions: completion
                .items
                .iter()
                .map(|item| Suggestion {
                    _id: item.label.clone(),
                    display_text: item.label.clone(),
                    _description: None,
                    suggestion_type: item.suggestion_type.clone(),
                })
                .collect(),
            selected: completion.selected_index,
        }
    }

    pub fn is_visible(&self) -> bool {
        !self.suggestions.is_empty()
    }

    pub fn height(&self) -> u16 {
        if self.is_visible() {
            self.suggestions.len().min(5) as u16 + 1
        } else {
            0
        }
    }
}

impl InputArea {
    /// 建议区域所需高度（0 表示无需渲染）。
    pub fn suggestions_height(&self, completion: &InputCompletion) -> u16 {
        SuggestionViewState::from_completion(completion).height()
    }

    /// Render the suggestions dropdown in a dedicated area (above status bar)
    pub fn render_suggestions_in_area(
        &self,
        area: Rect,
        buf: &mut Buffer,
        suggestions: &SuggestionViewState,
    ) {
        if !suggestions.is_visible() {
            return;
        }

        let max_visible = 5;
        let max_cols = area.width as usize;
        let selected = suggestions.selected.unwrap_or(0);
        // Compute scroll offset so the selected item is always visible
        let scroll_offset = if selected >= max_visible {
            selected - max_visible + 1
        } else {
            0
        };
        for (i, suggestion) in suggestions
            .suggestions
            .iter()
            .skip(scroll_offset)
            .take(max_visible)
            .enumerate()
        {
            let is_selected = i + scroll_offset == selected;
            let y = area.y + i as u16;
            let bg_color = if is_selected {
                theme::SELECTION_BG
            } else {
                theme::SURFACE_ELEVATED
            };
            let fg_color = if is_selected {
                theme::SELECTION_FG
            } else {
                theme::TEXT
            };
            let text = format!(
                " {} {}",
                suggestion_icon(suggestion),
                suggestion.display_text
            );
            let (truncated, _) = truncate_unicode_width(&text, max_cols.saturating_sub(2));
            render_suggestion_row(
                area,
                buf,
                y,
                truncated,
                Style::default().fg(fg_color).bg(bg_color),
            );
        }
    }
}

fn suggestion_icon(suggestion: &Suggestion) -> &'static str {
    match suggestion.suggestion_type {
        SuggestionType::Command => "/",
        SuggestionType::File => "📄",
        SuggestionType::Directory => "📁",
        SuggestionType::Model => "🤖",
        SuggestionType::Session => ">",
    }
}

fn render_suggestion_row(area: Rect, buf: &mut Buffer, y: u16, text: &str, style: Style) {
    let max_cols = area.width as usize;
    let mut col: usize = 0;
    for ch in text.chars() {
        if col >= max_cols {
            break;
        }
        let ch_w = ch.width().unwrap_or(1);
        if col + ch_w > max_cols {
            fill_row(area, buf, y, col, style);
            return;
        }
        if area.x + (col as u16) < buf.area.width {
            buf[(area.x + col as u16, y)].set_char(ch).set_style(style);
        }
        if ch_w > 1 {
            let next_col = col + 1;
            if next_col < max_cols && area.x + (next_col as u16) < buf.area.width {
                buf[(area.x + next_col as u16, y)]
                    .set_char('\0')
                    .set_style(style);
            }
        }
        col += ch_w;
    }
    fill_row(area, buf, y, col, style);
}

fn fill_row(area: Rect, buf: &mut Buffer, y: u16, from_col: usize, style: Style) {
    for c in from_col..area.width as usize {
        if area.x + (c as u16) < buf.area.width {
            buf[(area.x + c as u16, y)].set_char(' ').set_style(style);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::model::input::completion_item::CompletionItem;

    #[test]
    fn test_suggestion_view_from_completion_maps_types_and_selection() {
        let mut completion = InputCompletion::default();
        completion.set_items(
            vec![CompletionItem::with_type(
                "src/main.rs",
                "src/main.rs",
                SuggestionType::File,
            )],
            "@src".to_string(),
        );

        let view = SuggestionViewState::from_completion(&completion);

        assert_eq!(view.selected, Some(0));
        assert_eq!(view.suggestions[0].display_text, "src/main.rs");
        assert!(matches!(
            view.suggestions[0].suggestion_type,
            SuggestionType::File
        ));
    }

    #[test]
    fn test_suggestion_view_height_caps_visible_rows() {
        let mut completion = InputCompletion::default();
        completion.set_items(
            (0..8)
                .map(|i| CompletionItem::new(format!("/cmd{i}"), format!("/cmd{i}")))
                .collect(),
            "/".to_string(),
        );

        let view = SuggestionViewState::from_completion(&completion);

        assert_eq!(view.height(), 6);
    }

    #[test]
    fn test_suggestion_view_empty_is_hidden() {
        let completion = InputCompletion::default();
        let view = SuggestionViewState::from_completion(&completion);

        assert!(!view.is_visible());
        assert_eq!(view.height(), 0);
    }
}
