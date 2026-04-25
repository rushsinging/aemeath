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
      /// 鼠标选中状态
      is_selecting: bool,
      selection_start: Option<(usize, usize)>, // (row, col) in textarea
      selection_end: Option<(usize, usize)>,   // (row, col) in textarea
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
            is_selecting: false,
            selection_start: None,
            selection_end: None,
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

        // 叠加选中高亮
        if let Some(((start_row, start_col), (end_row, end_col))) = self.get_normalized_selection() {
            let lines = self.textarea.lines();
            let selection_style = Style::default().bg(Color::Blue).fg(Color::White);

            for (row, line_text) in lines.iter().enumerate() {
                let line_chars: Vec<char> = line_text.chars().collect();
                let line_len = line_chars.len();

                // 计算本行的选中列范围
                if row < start_row || row > end_row {
                    continue;
                }
                if row == start_row && row == end_row && start_col == end_col {
                    continue;
                }

                let col_from = if row == start_row { start_col } else { 0 };
                let col_to = if row == end_row { end_col.min(line_len) } else { line_len };

                // 在 buf 上设置选中高亮
                let screen_y = inner_area.y + row as u16;
                if screen_y >= inner_area.bottom() {
                    break;
                }
                for c in col_from..col_to {
                    let screen_x = inner_area.x + c as u16;
                    if screen_x >= inner_area.right() {
                        break;
                    }
                    if let Some(cell) = buf.cell_mut((screen_x, screen_y)) {
                        let ch = cell.symbol().to_string();
                        cell.set_style(selection_style);
                        // 保留原来的字符
                        if !ch.is_empty() {
                            cell.set_symbol(&ch);
                        }
                    }
                }
            }
        }
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
  
    /// 开始选中。row/col 是相对于 input_area inner rect 的偏移
    pub fn start_selection(&mut self, row: u16, col: u16, inner_area: &Rect) {
        let ta_row = row.saturating_sub(inner_area.y) as usize;
        let ta_col = col.saturating_sub(inner_area.x) as usize;
        self.selection_start = Some((ta_row, ta_col));
        self.selection_end = Some((ta_row, ta_col));
        self.is_selecting = true;
    }
  
    /// 更新选中位置
    pub fn update_selection(&mut self, row: u16, col: u16, inner_area: &Rect) {
        if !self.is_selecting {
            return;
        }
        let ta_row = row.saturating_sub(inner_area.y) as usize;
        let ta_col = col.saturating_sub(inner_area.x) as usize;
        self.selection_end = Some((ta_row, ta_col));
    }
  
    /// 结束选中并复制到剪贴板
    pub fn end_selection(&mut self) -> Option<String> {
        self.is_selecting = false;
        let text = self.get_selected_text();
        if let Some(ref t) = text {
            self.copy_to_clipboard(t);
        }
        self.selection_start = None;
        self.selection_end = None;
        text
    }
  
    /// 获取选中的文本
    pub fn get_selected_text(&self) -> Option<String> {
        let (start_row, start_col) = self.selection_start?;
        let (end_row, end_col) = self.selection_end?;
  
        let (start_row, start_col, end_row, end_col) = 
            if start_row < end_row || (start_row == end_row && start_col < end_col) {
                (start_row, start_col, end_row, end_col)
            } else {
                (end_row, end_col, start_row, start_col)
            };
  
        if start_row == end_row && start_col == end_col {
            return None;
        }
  
        let lines = self.textarea.lines();
        let mut result = String::new();
  
        for row in start_row..=end_row {
            if row >= lines.len() {
                break;
            }
            let line_chars: Vec<char> = lines[row].chars().collect();
            let from = if row == start_row { start_col } else { 0 };
            let to = if row == end_row { end_col.min(line_chars.len()) } else { line_chars.len() };
            if row > start_row {
                result.push('\n');
            }
            result.extend(line_chars[from..to].iter());
        }
  
        if result.is_empty() { None } else { Some(result) }
    }
  
    /// 复制到剪贴板
    fn copy_to_clipboard(&self, text: &str) {
        use std::io::Write;
        if let Ok(mut child) = std::process::Command::new("pbcopy")
            .stdin(std::process::Stdio::piped()).spawn()
        {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(text.as_bytes());
            }
            let _ = child.wait();
        }
    }
  
    /// 清除选中
    pub fn clear_selection(&mut self) {
        self.selection_start = None;
        self.selection_end = None;
        self.is_selecting = false;
    }
  
    /// 是否正在选中
    pub fn is_selecting(&self) -> bool {
        self.is_selecting
    }
  
    /// 获取归一化的选中范围 (start <= end)
    fn get_normalized_selection(&self) -> Option<((usize, usize), (usize, usize))> {
        let (start_row, start_col) = self.selection_start?;
        let (end_row, end_col) = self.selection_end?;
        if start_row == end_row && start_col == end_col {
            return None;
        }
        if start_row < end_row || (start_row == end_row && start_col < end_col) {
            Some(((start_row, start_col), (end_row, end_col)))
        } else {
            Some(((end_row, end_col), (start_row, start_col)))
        }
    }

    /// 获取 inner area（textarea 的实际渲染区域，去掉 border）
    pub fn get_inner_area(&self, area: &Rect) -> Rect {
        let block = ratatui::widgets::Block::default()
            .borders(ratatui::widgets::Borders::ALL);
        block.inner(*area)
    }
}