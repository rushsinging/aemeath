use aemeath_core::string_idx::{char_to_byte, CharIdx, StrSlice};

use crate::tui::output_area::markdown;
use crate::tui::safe_text::{safe_char_slice, safe_str_slice_by_char};

impl super::OutputArea {
    /// 获取逻辑行总数（包括普通行 + task_status 虚拟行）
    fn total_virtual_line_count(&self) -> usize {
        self.lines.len() + self.task_status_lines.len()
    }

    /// 根据逻辑索引获取行文本内容。
    /// idx < self.lines.len() → 普通行；否则 → task_status_lines[i]
    fn get_line_content(&self, idx: usize) -> Option<String> {
        if let Some(rendered) = self.rendered_line_content.get(&idx) {
            return Some(rendered.clone());
        }
        if idx < self.lines.len() {
            return Some(self.lines[idx].content.clone());
        }
        let task_idx = idx - self.lines.len();
        self.task_status_lines
            .get(task_idx)
            .map(|s| format!("  {s}"))
    }

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
            if let Some(line) = self.get_line_content(logic_idx) {
                let byte_start = char_to_byte(&line, char_start);
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
            if let Some(line) = self.get_line_content(logic_idx) {
                let byte_start = char_to_byte(&line, char_start);
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

    /// Select the word at the given screen position
    pub fn select_word(&mut self, row: u16, col: u16, rect: &ratatui::layout::Rect) {
        let rel_row = row.saturating_sub(rect.y) as usize;
        let rel_col = col.saturating_sub(rect.x) as usize;

        if rel_row >= self.screen_line_map.len() {
            return;
        }

        let (logic_idx, char_start, _char_end) = self.screen_line_map[rel_row];
        let Some(line) = self.get_line_content(logic_idx) else {
            return;
        };

        let byte_start = char_to_byte(&line, char_start);
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
    }

    /// End selection and return selected text without performing clipboard side effects.
    pub fn end_selection(&mut self) -> Option<String> {
        self.is_selecting = false;
        let text = self.get_selected_text();
        self.selection_start = None;
        self.selection_end = None;
        text
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

        let total = self.total_virtual_line_count();
        let mut result = String::new();

        for logic_idx in start_logic..=end_logic {
            if logic_idx >= total {
                log::debug!(
                    "get_selected_text: logic_idx {} >= total {}, breaking",
                    logic_idx,
                    total
                );
                break;
            }

            let Some(content) = self.get_line_content(logic_idx) else {
                continue;
            };

            // 不同逻辑行之间加换行
            if logic_idx > start_logic {
                result.push('\n');
            }

            let chars: Vec<char> = content.chars().collect();
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
                safe_str_slice_by_char(&content, 0, 60)
            );
            result.extend(selected_chars.iter());
        }

        if result.is_empty() {
            None
        } else {
            // Bug #51: strip inline Markdown formatting so copied text
            // matches the rendered visual appearance, not raw source.
            Some(markdown::strip_inline_formatting(&result))
        }
    }

    /// Clear the current selection
    pub fn clear_selection(&mut self) {
        self.selection_start = None;
        self.selection_end = None;
        self.is_selecting = false;
    }

    /// Whether a selection drag is in progress
    pub fn is_selecting(&self) -> bool {
        self.is_selecting
    }
}

#[cfg(test)]
#[path = "selection_tests.rs"]
mod tests;
