use std::io::Write;

use aemeath_core::string_idx::{char_to_byte, CharIdx, StrSlice};

use crate::tui::safe_text::{safe_char_slice, safe_str_slice_by_char};

impl super::OutputArea {
    /// Start a selection at the given screen position
    /// row/col 是相对于输出区域 rect 的偏移
    pub fn start_selection(&mut self, row: u16, col: u16, rect: &ratatui::layout::Rect) {
        let rel_row = row.saturating_sub(rect.y) as usize;
        let rel_col = col.saturating_sub(rect.x) as usize;

        log::debug!(
            "sel: rect.y={}, row={}, rel_row={}, screen_map.len={}",
            rect.y,
            row,
            rel_row,
            self.screen_line_map.len()
        );

        // 将屏幕行映射到逻辑行+char偏移
        if rel_row < self.screen_line_map.len() {
            let (logic_idx, char_start, _char_end) = self.screen_line_map[rel_row];
            if logic_idx < self.lines.len() {
                let line = &self.lines[logic_idx].content;
                let byte_start = char_to_byte(line, char_start);
                let char_col = crate::tui::output_area::display::screen_col_to_char_idx(
                    line.bslice_from(byte_start),
                    rel_col,
                );
                self.selection_start = Some((logic_idx, char_start.add(char_col.as_usize())));
                self.selection_end = Some((logic_idx, char_start.add(char_col.as_usize())));
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
                    line.bslice_from(byte_start),
                    rel_col,
                );
                self.selection_end = Some((logic_idx, char_start.add(char_col.as_usize())));
            }
        } else {
            // 超出可见范围时，选到最后一个屏幕行对应的逻辑行末尾
            if let Some(&(_, _, char_end)) = self.screen_line_map.last() {
                let last_logic = self
                    .screen_line_map
                    .last()
                    .map(|(li, _, _)| *li)
                    .unwrap_or(0);
                self.selection_end = Some((last_logic, char_end));
            }
        }
    }

    /// End selection and copy selected text to clipboard
    pub fn end_selection(&mut self) -> Option<String> {
        self.is_selecting = false;
        let selected = self.get_selected_text();
        log::debug!(
            "end_selection: start={:?}, end={:?}, selected={:?}",
            self.selection_start,
            self.selection_end,
            selected
                .as_deref()
                .map(|s| safe_str_slice_by_char(s, 0, 100))
        );
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
            line.bslice_from(byte_start),
            rel_col,
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

        self.selection_start = Some((logic_idx, CharIdx::new(start)));
        self.selection_end = Some((logic_idx, CharIdx::new(end + 1)));
        self.is_selecting = true;

        if let Some(text) = self.get_selected_text() {
            self.copy_to_clipboard(&text);
        }
    }

    /// Get the selected text based on logic line coordinates
    pub fn get_selected_text(&self) -> Option<String> {
        let (start_logic, start_col) = self.selection_start?;
        let (end_logic, end_col) = self.selection_end?;

        let (start_logic, start_col, end_logic, end_col) =
            if start_logic < end_logic || (start_logic == end_logic && start_col < end_col) {
                (start_logic, start_col, end_logic, end_col)
            } else {
                (end_logic, end_col, start_logic, start_col)
            };

        if start_logic == end_logic && start_col == end_col {
            return None;
        }

        let mut result = String::new();

        for logic_idx in start_logic..=end_logic {
            if logic_idx >= self.lines.len() {
                log::debug!(
                    "get_selected_text: logic_idx {} >= lines len {}, breaking",
                    logic_idx,
                    self.lines.len()
                );
                break;
            }

            // 不同逻辑行之间加换行
            if logic_idx > start_logic {
                result.push('\n');
            }

            let chars: Vec<char> = self.lines[logic_idx].content.chars().collect();
            let from = if logic_idx == start_logic {
                start_col.as_usize().min(chars.len())
            } else {
                0
            };
            let to = if logic_idx == end_logic {
                end_col.as_usize().min(chars.len())
            } else {
                chars.len()
            };
            let selected_chars = safe_char_slice(&chars, from, to);
            if selected_chars.is_empty() {
                log::debug!(
                    "get_selected_text: empty clamped range logic={}, from={}, to={}, chars_len={}",
                    logic_idx,
                    from,
                    to,
                    chars.len()
                );
                continue;
            }
            log::debug!(
                "get_selected_text: logic={}, from={}, to={}, chars_len={}, content={:?}",
                logic_idx,
                from,
                to,
                chars.len(),
                safe_str_slice_by_char(&self.lines[logic_idx].content, 0, 60)
            );
            result.extend(selected_chars.iter());
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
            .stdin(std::process::Stdio::piped())
            .spawn()
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

#[cfg(test)]
mod tests {
    use super::super::{LineStyle, OutputArea, OutputLine};
    use aemeath_core::string_idx::CharIdx;

    #[test]
    fn test_get_selected_text_clamps_start_col_after_line_shrinks() {
        let mut output = OutputArea::new();
        output.push_line(OutputLine {
            content: "短".to_string(),
            style: LineStyle::Assistant,
            ..Default::default()
        });
        output.selection_start = Some((0, CharIdx::new(4)));
        output.selection_end = Some((0, CharIdx::new(6)));

        let selected = output.get_selected_text();

        assert_eq!(selected, None);
    }

    #[test]
    fn test_get_selected_text_skips_line_when_clamped_start_exceeds_end() {
        let mut output = OutputArea::new();
        output.push_line(OutputLine {
            content: "ab".to_string(),
            style: LineStyle::Assistant,
            ..Default::default()
        });
        output.selection_start = Some((0, CharIdx::new(4)));
        output.selection_end = Some((0, CharIdx::new(1)));

        let selected = output.get_selected_text();

        assert_eq!(selected, Some("b".to_string()));
    }
}
