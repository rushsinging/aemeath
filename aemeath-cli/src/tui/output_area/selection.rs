use std::io::Write;

use aemeath_core::string_idx::{char_to_byte, CharIdx, StrSlice};

impl super::OutputArea {
    /// Start a selection at the given screen position
    /// row/col 是相对于输出区域 rect 的偏移
    pub fn start_selection(&mut self, row: u16, col: u16, rect: &ratatui::layout::Rect) {
        let rel_row = row.saturating_sub(rect.y) as usize;
        let rel_col = col.saturating_sub(rect.x) as usize;

        // 将屏幕行映射到逻辑行+char偏移
        if rel_row < self.screen_line_map.len() {
            let (logic_idx, char_start, _char_end) = self.screen_line_map[rel_row];
            if logic_idx < self.lines.len() {
                let line = &self.lines[logic_idx].content;
                let byte_start = char_to_byte(line, char_start);
                let char_col = crate::tui::output_area::display::screen_col_to_char_idx(
                    line.bslice_from(byte_start), rel_col,
                );
                self.selection_start = Some((rel_row, char_start.add(char_col.as_usize())));
                self.selection_end = Some((rel_row, char_start.add(char_col.as_usize())));
            }
        }
        self.is_selecting = true;
    }

    /// Update selection end position during drag
    pub fn update_selection(&mut self, row: u16, col: u16, rect: &ratatui::layout::Rect) {
        if !self.is_selecting {
            return;
        }
        let rel_row = row.saturating_sub(rect.y) as usize;
        let rel_col = col.saturating_sub(rect.x) as usize;

        if rel_row < self.screen_line_map.len() {
            let (logic_idx, char_start, _char_end) = self.screen_line_map[rel_row];
            if logic_idx < self.lines.len() {
                let line = &self.lines[logic_idx].content;
                let byte_start = char_to_byte(line, char_start);
                let char_col = crate::tui::output_area::display::screen_col_to_char_idx(
                    line.bslice_from(byte_start), rel_col,
                );
                self.selection_end = Some((rel_row, char_start.add(char_col.as_usize())));
            }
        } else {
            // 超出可见范围时，选到最后一个屏幕行的末尾
            if let Some(&(_, _, char_end)) = self.screen_line_map.last() {
                self.selection_end = Some((self.screen_line_map.len().saturating_sub(1), char_end));
            }
        }
    }

    /// End selection and copy selected text to clipboard
    pub fn end_selection(&mut self) -> Option<String> {
        self.is_selecting = false;
        let selected = self.get_selected_text();
        if let Some(ref text) = selected {
            self.copy_to_clipboard(text);
        }
        selected
    }

    /// Select the word at the given screen position
    pub fn select_word(&mut self, row: u16, col: u16, rect: &ratatui::layout::Rect) {
        let rel_row = row.saturating_sub(rect.y) as usize;
        let rel_col = col.saturating_sub(rect.x) as usize;

        if rel_row >= self.screen_line_map.len() {
            return;
        }

        let (logic_idx, char_start, _char_end) = self.screen_line_map[rel_row];
        if logic_idx >= self.lines.len() {
            return;
        }

        let line = &self.lines[logic_idx].content;
        let byte_start = char_to_byte(line, char_start);
        let char_col = crate::tui::output_area::display::screen_col_to_char_idx(
            line.bslice_from(byte_start), rel_col,
        );
        let abs_char_idx = char_start.add(char_col.as_usize());

        let chars: Vec<char> = line.chars().collect();
        if chars.is_empty() {
            return;
        }

        let idx = abs_char_idx.as_usize().min(chars.len() - 1);
        let is_word_char = |c: char| c.is_alphanumeric() || c == '_';

        let mut start = idx;
        let mut end = idx;

        if is_word_char(chars[idx]) {
            while start > 0 && is_word_char(chars[start - 1]) {
                start -= 1;
            }
            while end < chars.len() - 1 && is_word_char(chars[end + 1]) {
                end += 1;
            }
        }

        self.selection_start = Some((rel_row, CharIdx::new(start)));
        self.selection_end = Some((rel_row, CharIdx::new(end + 1)));
        self.is_selecting = true;

        if let Some(text) = self.get_selected_text() {
            self.copy_to_clipboard(&text);
        }
    }

    /// Get the selected text based on screen line coordinates
    pub fn get_selected_text(&self) -> Option<String> {
        let (start_screen, start_col) = self.selection_start?;
        let (end_screen, end_col) = self.selection_end?;

        let (start_screen, start_col, end_screen, end_col) = if start_screen < end_screen
            || (start_screen == end_screen && start_col < end_col)
        {
            (start_screen, start_col, end_screen, end_col)
        } else {
            (end_screen, end_col, start_screen, start_col)
        };

        if start_screen == end_screen && start_col == end_col {
            return None;
        }

        let mut result = String::new();
        let mut prev_logic_idx = None;

        for screen_idx in start_screen..=end_screen {
            if screen_idx >= self.screen_line_map.len() {
                break;
            }
            let (logic_idx, chunk_start, _chunk_end) = self.screen_line_map[screen_idx];
            if logic_idx >= self.lines.len() {
                break;
            }

            // 不同逻辑行之间加换行
            if let Some(prev) = prev_logic_idx {
                if logic_idx != prev {
                    result.push('\n');
                }
            }
            prev_logic_idx = Some(logic_idx);

            let chars: Vec<char> = self.lines[logic_idx].content.chars().collect();
            let from = if screen_idx == start_screen {
                start_col.max(chunk_start).as_usize()
            } else {
                chunk_start.as_usize()
            };
            let to = if screen_idx == end_screen {
                end_col.as_usize().min(chars.len())
            } else {
                chars.len()
            };
            result.extend(chars[from..to].iter());
        }

        if result.is_empty() {
            None
        } else {
            Some(result)
        }
    }

    /// Copy text to system clipboard
    fn copy_to_clipboard(&self, text: &str) {
        if let Ok(mut child) = std::process::Command::new("pbcopy")
            .stdin(std::process::Stdio::piped()).spawn()
        {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(text.as_bytes());
            }
            let _ = child.wait();
        }
    }

    /// Clear the current selection
    pub fn clear_selection(&mut self) {
        self.selection_start = None;
        self.selection_end = None;
        self.is_selecting = false;
    }
}
