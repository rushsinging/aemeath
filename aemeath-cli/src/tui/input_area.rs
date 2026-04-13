use crate::tui::completion::{Suggestion, SuggestionType};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    widgets::{Block, Borders, Widget},
};
use tui_textarea::{CursorMove, TextArea};

/// The input area with a multi-line text editor and autocomplete
pub struct InputArea {
    textarea: TextArea<'static>,
    focused: bool,
    pending_images: usize,
    /// Active suggestions for autocomplete
    suggestions: Vec<Suggestion>,
    /// Currently selected suggestion index (-1 means none)
    selected_suggestion: i32,
    /// Whether suggestions are visible
    show_suggestions: bool,
    /// Command history
    history: Vec<String>,
    /// Current position in history (None means not browsing history)
    history_index: Option<usize>,
    /// Saved input before browsing history (to restore when navigating back)
    saved_input: String,
}

impl Default for InputArea {
    fn default() -> Self {
        Self::new()
    }
}

impl InputArea {
    pub fn new() -> Self {
        let mut textarea = TextArea::default();
        textarea.set_placeholder_text("Type a message... (Enter to send, Alt+Enter for new line)");
        textarea.set_cursor_line_style(Style::default().bg(Color::Reset));
        textarea.set_cursor_style(Style::default().bg(Color::Cyan));
        
        Self {
            textarea,
            focused: true,
            pending_images: 0,
            suggestions: Vec::new(),
            selected_suggestion: -1,
            show_suggestions: false,
            history: Vec::new(),
            history_index: None,
            saved_input: String::new(),
        }
    }

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

    /// Add a message to history
    pub fn add_history(&mut self, text: &str) {
        // Don't add empty messages or duplicates
        if text.is_empty() {
            return;
        }
        // Remove duplicate if exists (move to end)
        if let Some(pos) = self.history.iter().position(|s| s == text) {
            self.history.remove(pos);
        }
        self.history.push(text.to_string());
        // Limit history size
        const MAX_HISTORY: usize = 100;
        if self.history.len() > MAX_HISTORY {
            self.history.remove(0);
        }
    }

    /// Navigate up in history (older messages)
    pub fn history_up(&mut self) {
        if self.history.is_empty() {
            return;
        }
        
        // Save current input if starting to browse history
        if self.history_index.is_none() {
            self.saved_input = self.get_text();
            self.history_index = Some(self.history.len());
        }
        
        // Move to older message (decrease index)
        if let Some(idx) = self.history_index {
            if idx > 0 {
                let new_idx = idx - 1;
                self.history_index = Some(new_idx);
                let text = self.history[new_idx].clone();
                self.set_text(&text);
            }
        }
    }

    /// Navigate down in history (newer messages)
    pub fn history_down(&mut self) {
        if self.history.is_empty() || self.history_index.is_none() {
            return;
        }
        
        if let Some(idx) = self.history_index {
            if idx < self.history.len() - 1 {
                let new_idx = idx + 1;
                self.history_index = Some(new_idx);
                let text = self.history[new_idx].clone();
                self.set_text(&text);
            } else {
                // Reached the end, restore saved input
                let text = self.saved_input.clone();
                self.set_text(&text);
                self.history_index = None;
            }
        }
    }

    /// Reset history navigation
    pub fn reset_history_nav(&mut self) {
        self.history_index = None;
        self.saved_input.clear();
    }

    /// Get the current input text
    pub fn get_text(&self) -> String {
        self.textarea.lines().join("\n")
    }

    /// Clear the input
    pub fn clear(&mut self) {
        self.textarea = TextArea::default();
        self.textarea.set_placeholder_text("Type a message... (Enter to send, Alt+Enter for new line)");
        self.textarea.set_cursor_line_style(Style::default().bg(Color::Reset));
        self.textarea.set_cursor_style(Style::default().bg(Color::Cyan));
        self.clear_suggestions();
        self.reset_history_nav();
    }

    /// Handle a character input
    pub fn input(&mut self, c: char) {
        self.textarea.insert_char(c);
        self.show_suggestions = false; // Hide suggestions while typing
        self.reset_history_nav(); // Reset history navigation when typing
    }

    /// Handle a backspace
    pub fn backspace(&mut self) {
        self.textarea.delete_char();
        self.show_suggestions = false;
        self.reset_history_nav(); // Reset history navigation when deleting
    }

    /// Handle enter key - returns true if should send
    pub fn enter(&mut self, alt: bool) -> bool {
        if alt {
            // Insert newline
            self.textarea.insert_newline();
            false
        } else if self.show_suggestions {
            // Accept suggestion if showing
            self.accept_suggestion();
            false // Don't send, just accept suggestion
        } else {
            // Send message
            true
        }
    }

    /// Move cursor left
    pub fn move_left(&mut self) {
        self.textarea.move_cursor(CursorMove::Back);
        self.show_suggestions = false;
        self.reset_history_nav();
    }

    /// Move cursor right
    pub fn move_right(&mut self) {
        self.textarea.move_cursor(CursorMove::Forward);
        self.show_suggestions = false;
        self.reset_history_nav();
    }

    /// Move cursor up (or select previous suggestion, or browse history)
    pub fn move_up(&mut self) {
        if self.select_previous() {
            // Suggestion was selected
        } else {
            // Check if cursor is at the first line
            let (row, _) = self.textarea.cursor();
            if row == 0 {
                // Browse history instead of moving cursor
                self.history_up();
            } else {
                self.textarea.move_cursor(CursorMove::Up);
            }
        }
    }

    /// Move cursor down (or select next suggestion, or browse history)
    pub fn move_down(&mut self) {
        if self.select_next() {
            // Suggestion was selected
        } else {
            // Check if cursor is at the last line
            let (row, _) = self.textarea.cursor();
            let line_count = self.textarea.lines().len();
            if row >= line_count - 1 {
                // Browse history instead of moving cursor
                self.history_down();
            } else {
                self.textarea.move_cursor(CursorMove::Down);
            }
        }
    }

    /// Move cursor to start of line
    pub fn move_home(&mut self) {
        self.textarea.move_cursor(CursorMove::Head);
        self.show_suggestions = false;
    }

    /// Move cursor to end of line
    pub fn move_end(&mut self) {
        self.textarea.move_cursor(CursorMove::End);
        self.show_suggestions = false;
    }

    /// Delete word
    pub fn delete_word(&mut self) {
        self.textarea.delete_word();
        self.show_suggestions = false;
    }

    /// Set pending images count
    pub fn set_pending_images(&mut self, count: usize) {
        self.pending_images = count;
    }

    /// Check if input is empty
    pub fn is_empty(&self) -> bool {
        self.textarea.lines().iter().all(|line| line.is_empty())
    }

    /// Get cursor position (line, column)
    pub fn cursor_position(&self) -> (usize, usize) {
        self.textarea.cursor()
    }

    /// Set text content
    pub fn set_text(&mut self, text: &str) {
        self.textarea.delete_line_by_head();
        for line in text.lines() {
            self.textarea.insert_str(line);
        }
    }

    /// Render the input area
    pub fn render(&mut self, area: Rect, buf: &mut Buffer) {
        // Build title with pending images indicator
        let title = if self.pending_images > 0 {
            format!(" Input [{} image(s) pending] ", self.pending_images)
        } else {
            " Input ".to_string()
        };

        let border_style = if self.focused {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        let block = Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_style(border_style);

        let inner_area = block.inner(area);
        block.render(area, buf);

        // Render textarea
        self.textarea.set_block(Block::default());
        self.textarea.render(inner_area, buf);
    }

    /// Render the suggestions dropdown in a dedicated area (above status bar)
    pub fn render_suggestions_in_area(&self, area: Rect, buf: &mut Buffer) {
        if !self.show_suggestions || self.suggestions.is_empty() {
            return;
        }

        let max_visible = 5;
        let _visible_count = self.suggestions.len().min(max_visible);
        
        // Render suggestion items
        for (i, suggestion) in self.suggestions.iter().take(max_visible).enumerate() {
            let is_selected = i as i32 == self.selected_suggestion;
            let y = area.y + i as u16;
            
            // Background color for selected item
            let bg_color = if is_selected { Color::Cyan } else { Color::Reset };
            let fg_color = if is_selected { Color::Black } else { Color::White };
            
            // Icon based on suggestion type
            let icon = match suggestion.suggestion_type {
                SuggestionType::Command => "/",
                SuggestionType::File => "📄",
                SuggestionType::Directory => "📁",
                SuggestionType::Model => "🤖",
            };

            // Render the suggestion text
            let text = format!(" {} {}", icon, suggestion.display_text);
            let truncated = if text.len() > area.width as usize - 2 {
                text.chars().take(area.width as usize - 4).collect::<String>() + ".."
            } else {
                text
            };

            // Fill the entire row with background color
            for x in 0..area.width {
                let x_usize = x as usize;
                if x as u16 + area.x < buf.area.width {
                    let ch = if x_usize < truncated.len() { 
                        truncated.chars().nth(x_usize).unwrap_or(' ') 
                    } else { 
                        ' ' 
                    };
                    buf[(area.x + x, y)]
                        .set_char(ch)
                        .set_style(Style::default().fg(fg_color).bg(bg_color));
                }
            }
        }
    }
}