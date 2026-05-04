use crate::tui::completion::{Suggestion, SuggestionType};
use crate::tui::safe_text::safe_char_slice;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    widgets::{Block, Borders, Widget},
};
use tui_textarea::{CursorMove, TextArea};
use unicode_width::UnicodeWidthChar;

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
    /// textarea 渲染区域宽度（用于自动换行）
    content_width: u16,
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
            content_width: 0,
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
        self.textarea
            .set_placeholder_text("Type a message... (Enter to send, Alt+Enter for new line)");
        self.textarea
            .set_cursor_line_style(Style::default().bg(Color::Reset));
        self.textarea
            .set_cursor_style(Style::default().bg(Color::Cyan));
        self.clear_suggestions();
        self.reset_history_nav();
    }

    /// Handle a character input
    pub fn input(&mut self, c: char) {
        self.textarea.insert_char(c);
        self.show_suggestions = false; // Hide suggestions while typing
        self.reset_history_nav(); // Reset history navigation when typing
        self.auto_wrap_current_line();
    }

    /// 当前行超过视觉宽度时，在合适位置插入换行符
    fn auto_wrap_current_line(&mut self) {
        let max_w = self.content_width as usize;
        if max_w < 10 {
            // 宽度太小不折行
            return;
        }
        let (row, col) = self.textarea.cursor();
        let lines = self.textarea.lines();
        if row >= lines.len() {
            return;
        }
        let line = &lines[row];
        let chars: Vec<char> = line.chars().collect();
        let display_width: usize = chars
            .iter()
            .map(|c| {
                if *c == '\t' {
                    4
                } else {
                    c.width().unwrap_or(1)
                }
            })
            .sum();
        if display_width <= max_w {
            return;
        }

        // 找最佳折行点：最后一个不超过 max_w 的空格
        let mut best_break = 0;
        let mut current_width = 0;
        for (i, ch) in chars.iter().enumerate() {
            let w = if *ch == '\t' {
                4
            } else {
                ch.width().unwrap_or(1)
            };
            if current_width + w > max_w {
                break;
            }
            if *ch == ' ' {
                best_break = i + 1;
            }
            current_width += w;
        }

        if best_break == 0 {
            // 没有空格，硬折行
            let mut w = 0;
            for (i, ch) in chars.iter().enumerate() {
                w += if *ch == '\t' {
                    4
                } else {
                    ch.width().unwrap_or(1)
                };
                if w > max_w {
                    best_break = i;
                    break;
                }
            }
            if best_break == 0 {
                return;
            }
        }

        // 用文本操作实现折行：
        // 1. 移到行首
        // 2. 右移 best_break 个字符（选中 before 部分）
        // 3. 删除行尾
        // 4. 输入换行
        // 5. 输入 after 部分
        // 6. 恢复光标
        self.textarea.move_cursor(CursorMove::Head);

        // 选中从行首到 best_break 位置的文本，删掉行尾
        // 先移到 best_break 位置
        for _ in 0..best_break {
            self.textarea.move_cursor(CursorMove::Forward);
        }
        // 删掉光标到行尾的内容（保存到 after）
        let after: String = safe_char_slice(&chars, best_break, chars.len())
            .iter()
            .collect();
        self.textarea.delete_line_by_end();
        // 插入换行和 after
        self.textarea.insert_newline();
        self.textarea.insert_str(&after);

        // 恢复光标位置
        if col >= best_break {
            // 光标在 after 部分：移到新行的 col - best_break 位置
            let new_col = col - best_break;
            self.textarea.move_cursor(CursorMove::Head);
            for _ in 0..new_col {
                self.textarea.move_cursor(CursorMove::Forward);
            }
        } else {
            // 光标在 before 部分：回到上一行的 col 位置
            self.textarea.move_cursor(CursorMove::Up);
            self.textarea.move_cursor(CursorMove::Head);
            for _ in 0..col {
                self.textarea.move_cursor(CursorMove::Forward);
            }
        }
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
        self.content_width = inner_area.width;
        block.render(area, buf);

        // Render textarea
        self.textarea.set_block(Block::default());
        self.textarea.render(inner_area, buf);

        // 叠加选中高亮
        if let Some(((start_row, start_col), (end_row, end_col))) = self.get_normalized_selection()
        {
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
                let col_to = if row == end_row {
                    end_col.min(line_len)
                } else {
                    line_len
                };

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
        let max_cols = area.width as usize;

        // Render suggestion items
        for (i, suggestion) in self.suggestions.iter().take(max_visible).enumerate() {
            let is_selected = i as i32 == self.selected_suggestion;
            let y = area.y + i as u16;

            // Background color for selected item
            let bg_color = if is_selected {
                Color::Cyan
            } else {
                Color::Reset
            };
            let fg_color = if is_selected {
                Color::Black
            } else {
                Color::White
            };

            // Icon based on suggestion type
            let icon = match suggestion.suggestion_type {
                SuggestionType::Command => "/",
                SuggestionType::File => "📄",
                SuggestionType::Directory => "📁",
                SuggestionType::Model => "🤖",
                SuggestionType::Session => ">",
            };

            // Build the suggestion text, truncate by display width
            let text = format!(" {} {}", icon, suggestion.display_text);
            let truncated = crate::tui::output_area::display::truncate_unicode_width(
                &text,
                max_cols.saturating_sub(2),
            );

            // Fill the entire row: walk chars by display width, not by char index
            let style = Style::default().fg(fg_color).bg(bg_color);
            let mut col: usize = 0;
            for ch in truncated.chars() {
                if col >= max_cols {
                    break;
                }
                let ch_w = ch.width().unwrap_or(1) as usize;
                // Wide char that would overflow → fill remaining with spaces and stop
                if col + ch_w > max_cols {
                    for c in col..max_cols {
                        if area.x + (c as u16) < buf.area.width {
                            buf[(area.x + c as u16, y)].set_char(' ').set_style(style);
                        }
                    }
                    col = max_cols;
                    break;
                }
                // Write the char at the current column
                if area.x + (col as u16) < buf.area.width {
                    buf[(area.x + col as u16, y)].set_char(ch).set_style(style);
                }
                // For wide chars (2 cols), fill the next cell with empty marker
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
            // Fill remaining columns with spaces
            for c in col..max_cols {
                if area.x + (c as u16) < buf.area.width {
                    buf[(area.x + c as u16, y)].set_char(' ').set_style(style);
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
            let to = if row == end_row {
                end_col.min(line_chars.len())
            } else {
                line_chars.len()
            };
            if row > start_row {
                result.push('\n');
            }
            result.extend(line_chars[from..to].iter());
        }

        if result.is_empty() {
            None
        } else {
            Some(result)
        }
    }

    /// 复制到剪贴板
    fn copy_to_clipboard(&self, text: &str) {
        use std::io::Write;
        if let Ok(mut child) = std::process::Command::new("pbcopy")
            .stdin(std::process::Stdio::piped())
            .spawn()
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
        let block = ratatui::widgets::Block::default().borders(ratatui::widgets::Borders::ALL);
        block.inner(*area)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;

    #[test]
    fn test_auto_wrap_current_line_handles_cjk_without_panic() {
        let mut input = InputArea::new();
        let area = Rect {
            x: 0,
            y: 0,
            width: 12,
            height: 3,
        };
        let mut buf = Buffer::empty(area);
        input.render(area, &mut buf);

        for ch in "你好世界你好世界".chars() {
            input.input(ch);
        }

        let text = input.get_text();
        assert!(text.contains('你'));
        assert!(text.contains('界'));
    }

    #[test]
    fn test_auto_wrap_current_line_handles_emoji_without_panic() {
        let mut input = InputArea::new();
        let area = Rect {
            x: 0,
            y: 0,
            width: 12,
            height: 3,
        };
        let mut buf = Buffer::empty(area);
        input.render(area, &mut buf);

        for ch in "a🚀b🚀c🚀d🚀e".chars() {
            input.input(ch);
        }

        let text = input.get_text();
        assert!(text.contains('🚀'));
        assert!(text.contains('e'));
    }
}
