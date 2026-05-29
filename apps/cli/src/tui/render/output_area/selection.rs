use sdk::{char_to_byte, CharIdx, StrSlice};

use crate::tui::render::display::safe_text::{safe_char_slice, safe_str_slice_by_char};

impl super::OutputArea {
    /// 获取逻辑行总数（包括 document 行 + task_status 虚拟行）
    fn total_virtual_line_count(&self) -> usize {
        self.document.total_lines() + self.task_status_lines.len()
    }

    /// 逻辑行的前导 gutter 显示列数（不进 plain）。task_status 等虚拟行无 gutter 返回 0。
    /// 点击列 → plain 字符映射需先减去此宽度（gutter 是 chrome，不可选）。
    fn gutter_cols_for_line(&self, idx: usize) -> usize {
        self.document
            .iter_lines()
            .nth(idx)
            .map(|line| line.gutter_cols)
            .unwrap_or(0)
    }

    /// 根据逻辑索引获取行文本内容。
    /// idx < document.total_lines() → document 行；否则 → task_status_lines[i]
    pub fn get_line_content(&mut self, idx: usize) -> Option<String> {
        if let Some(rendered) = self.rendered_line_content.get(&idx) {
            return Some(rendered.clone());
        }
        if let Some(line) = self.document.iter_lines().nth(idx) {
            return Some(line.plain.clone());
        }
        let task_idx = idx - self.document.total_lines();
        self.task_status_lines
            .get(task_idx)
            .map(|s| format!("  {s}"))
    }

    /// 屏幕坐标 → 选区锚点 `(逻辑行, plain CharIdx)`（#63 坐标系）的纯换算。
    ///
    /// 只读：查 `screen_line_map` + gutter_cols 列补偿 + `screen_col_to_char_idx`，
    /// 不改 widget 选区状态。供 mouse_handler 折算后写入 view_state 选区真相。
    /// row/col 为终端绝对坐标，rect 为输出区。映射失败（行超界/无行内容）返回 None。
    pub fn screen_to_anchor(
        &mut self,
        row: u16,
        col: u16,
        rect: &ratatui::layout::Rect,
    ) -> Option<(usize, CharIdx)> {
        let rel_row = row.saturating_sub(rect.y) as usize;
        let rel_col = col.saturating_sub(rect.x) as usize;
        if rel_row >= self.screen_line_map.len() {
            return None;
        }
        let (logic_idx, char_start, _char_end) = self.screen_line_map[rel_row];
        // 减去 gutter 显示列：gutter 不进 plain，点击 gutter 区间映射到 plain 字符 0。
        let content_col = rel_col.saturating_sub(self.gutter_cols_for_line(logic_idx));
        let line = self.get_line_content(logic_idx)?;
        let byte_start = char_to_byte(&line, char_start);
        let char_col = crate::tui::render::output_area::display::screen_col_to_char_idx(
            line.bslice_from(byte_start),
            content_col,
        );
        Some((logic_idx, char_start.advance(char_col.as_usize())))
    }

    /// 拖拽超出可见范围时的兜底锚点：最后一个屏幕行对应逻辑行的末尾。
    /// 只读，供 mouse_handler 在 `screen_to_anchor` 返回 None 时（行超界）补位。
    pub fn last_visible_anchor(&self) -> Option<(usize, CharIdx)> {
        self.screen_line_map
            .last()
            .map(|&(logic_idx, _, char_end)| (logic_idx, char_end))
    }

    /// 屏幕坐标 → 整词边界 `(逻辑行, word_start, word_end)`（半开区间）的纯换算。
    ///
    /// 只读：在 `screen_to_anchor` 命中的字符位置向两侧扫描 word-char（字母数字/下划线）。
    /// 非 word-char 命中点返回单字符词。映射失败返回 None。不改 widget 选区状态。
    pub fn word_bounds_at(
        &mut self,
        row: u16,
        col: u16,
        rect: &ratatui::layout::Rect,
    ) -> Option<(usize, CharIdx, CharIdx)> {
        let (logic_idx, abs_char_idx) = self.screen_to_anchor(row, col, rect)?;
        let line = self.get_line_content(logic_idx)?;
        let chars: Vec<char> = line.chars().collect();
        if chars.is_empty() {
            return None;
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

        Some((logic_idx, CharIdx::new(start), CharIdx::new(end + 1)))
    }

    /// Start a selection at the given screen position
    /// row/col 是相对于输出区域 rect 的偏移
    pub fn start_selection(&mut self, row: u16, col: u16, rect: &ratatui::layout::Rect) {
        if let Some((logic_idx, anchor)) = self.screen_to_anchor(row, col, rect) {
            self.selection_start = Some((logic_idx, anchor));
            self.selection_end = Some((logic_idx, anchor));
        }
        self.is_selecting = true;
    }

    /// Update selection end position during drag
    pub fn update_selection(&mut self, row: u16, col: u16, rect: &ratatui::layout::Rect) {
        if !self.is_selecting {
            return;
        }
        if let Some(anchor) = self.screen_to_anchor(row, col, rect) {
            self.selection_end = Some(anchor);
        } else if let Some(anchor) = self.last_visible_anchor() {
            // 超出可见范围时，选到最后一个屏幕行对应的逻辑行末尾
            self.selection_end = Some(anchor);
        }
    }

    /// Select the word at the given screen position
    pub fn select_word(&mut self, row: u16, col: u16, rect: &ratatui::layout::Rect) {
        if let Some((logic_idx, word_start, word_end)) = self.word_bounds_at(row, col, rect) {
            self.selection_start = Some((logic_idx, word_start));
            self.selection_end = Some((logic_idx, word_end));
            self.is_selecting = true;
        }
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
    pub fn get_selected_text(&mut self) -> Option<String> {
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

        let total = self
            .document
            .total_lines()
            .max(self.total_virtual_line_count());
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
            Some(result)
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
#[path = "../output/selection_tests.rs"]
mod tests;

#[cfg(test)]
mod document_selection_tests {
    use super::super::OutputArea;
    use crate::tui::render::output::rendered::{RenderedBlock, RenderedDocument, RenderedLine};
    use ratatui::text::Span;
    use sdk::CharIdx;

    #[test]
    fn test_copy_selection_returns_plain_chars_across_lines() {
        let mut area = OutputArea::new();
        area.set_document(RenderedDocument {
            blocks: vec![RenderedBlock {
                block_id: "a".into(),
                lines: vec![
                    RenderedLine::with_plain(vec![Span::raw("**bold**")], "bold".into()),
                    RenderedLine::new(vec![Span::raw("世界")]),
                ],
            }],
        });
        area.set_selection_for_test((0, CharIdx::new(0)), (1, CharIdx::new(2)));
        let copied = area.get_selected_text();

        assert_eq!(copied.as_deref(), Some("bold\n世界"));
    }
}
