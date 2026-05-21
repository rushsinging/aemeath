use super::InputArea;
use crate::tui::completion::{Suggestion, SuggestionType};
use crate::tui::safe_text::truncate_unicode_width;
use crate::tui::theme;
use ratatui::{buffer::Buffer, layout::Rect, style::Style};
use unicode_width::UnicodeWidthChar;

impl InputArea {
    /// Set suggestions for autocomplete
    pub fn set_suggestions(&mut self, suggestions: Vec<Suggestion>) {
        self.selected_suggestion = if suggestions.is_empty() { -1 } else { 0 };
        self.show_suggestions = !suggestions.is_empty();
        self.suggestions = suggestions;
    }

    /// Clear suggestions
    pub fn clear_suggestions(&mut self) {
        self.suggestions.clear();
        self.selected_suggestion = -1;
        self.show_suggestions = false;
    }

    /// Get current suggestions
    pub fn get_suggestions(&self) -> &[Suggestion] {
        &self.suggestions
    }

    /// Move selection up in suggestions
    pub fn select_previous(&mut self) -> bool {
        if self.show_suggestions && !self.suggestions.is_empty() {
            if self.selected_suggestion > 0 {
                self.selected_suggestion -= 1;
            } else {
                self.selected_suggestion = self.suggestions.len() as i32 - 1;
            }
            true
        } else {
            false
        }
    }

    /// Move selection down in suggestions
    pub fn select_next(&mut self) -> bool {
        if self.show_suggestions && !self.suggestions.is_empty() {
            if self.selected_suggestion < self.suggestions.len() as i32 - 1 {
                self.selected_suggestion += 1;
            } else {
                self.selected_suggestion = 0;
            }
            true
        } else {
            false
        }
    }

    /// Accept current suggestion
    pub fn accept_suggestion(&mut self) -> Option<Suggestion> {
        if self.show_suggestions && self.selected_suggestion >= 0 {
            let idx = self.selected_suggestion as usize;
            if idx < self.suggestions.len() {
                let suggestion = self.suggestions[idx].clone();
                self.clear_suggestions();
                return Some(suggestion);
            }
        }
        None
    }

    /// Check if suggestions are showing
    pub fn is_showing_suggestions(&self) -> bool {
        self.show_suggestions
    }

    /// Render the suggestions dropdown in a dedicated area (above status bar)
    pub fn render_suggestions_in_area(&self, area: Rect, buf: &mut Buffer) {
        if !self.show_suggestions || self.suggestions.is_empty() {
            return;
        }

        let max_visible = 5;
        let max_cols = area.width as usize;
        let selected = if self.selected_suggestion >= 0 {
            self.selected_suggestion as usize
        } else {
            0
        };
        // Compute scroll offset so the selected item is always visible
        let scroll_offset = if selected >= max_visible {
            selected - max_visible + 1
        } else {
            0
        };
        for (i, suggestion) in self
            .suggestions
            .iter()
            .skip(scroll_offset)
            .take(max_visible)
            .enumerate()
        {
            let is_selected = (i + scroll_offset) as i32 == self.selected_suggestion;
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
