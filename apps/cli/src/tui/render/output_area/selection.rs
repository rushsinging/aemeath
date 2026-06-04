use sdk::{char_to_byte, CharIdx, StrSlice};

use crate::tui::render::display::safe_text::{safe_char_slice, safe_str_slice_by_char};
use crate::tui::view_model::LiveStatusViewModel;
use crate::tui::view_state::OutputViewState;

impl super::OutputArea {
    /// 获取逻辑行总数（包括 document 行 + task_status 虚拟行）
    fn total_virtual_line_count(&self, live_status: &LiveStatusViewModel) -> usize {
        self.document.total_lines() + live_status.task_lines.len()
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
    /// idx < document.total_lines() → document 行；否则 → live status task_lines[i]
    pub fn get_line_content(
        &self,
        idx: usize,
        live_status: &LiveStatusViewModel,
    ) -> Option<String> {
        if let Some(rendered) = self.rendered_line_content.get(&idx) {
            return Some(rendered.clone());
        }
        if let Some(line) = self.document.iter_lines().nth(idx) {
            return Some(line.plain.clone());
        }
        let task_idx = idx - self.document.total_lines();
        live_status
            .task_lines
            .get(task_idx)
            .map(|s| format!("  {s}"))
    }

    /// 屏幕坐标 → 选区锚点 `(逻辑行, plain CharIdx)`（#63 坐标系）的纯换算。
    ///
    /// 只读：查 `screen_line_map` + gutter_cols 列补偿 + `screen_col_to_char_idx`，
    /// 不改 widget 选区状态。供 mouse_handler 折算后写入 view_state 选区真相。
    /// row/col 为终端绝对坐标，rect 为输出区。映射失败（行超界/无行内容）返回 None。
    pub fn screen_to_anchor(
        &self,
        row: u16,
        col: u16,
        rect: &ratatui::layout::Rect,
        live_status: &LiveStatusViewModel,
    ) -> Option<(usize, CharIdx)> {
        let rel_row = row.saturating_sub(rect.y) as usize;
        let rel_col = col.saturating_sub(rect.x) as usize;
        if rel_row >= self.screen_line_map.len() {
            return None;
        }
        let (logic_idx, char_start, _char_end) = self.screen_line_map.get(rel_row).copied()?;
        // 减去 gutter 显示列：gutter 不进 plain，点击 gutter 区间映射到 plain 字符 0。
        let content_col = rel_col.saturating_sub(self.gutter_cols_for_line(logic_idx));
        let line = self.get_line_content(logic_idx, live_status)?;
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
        &self,
        row: u16,
        col: u16,
        rect: &ratatui::layout::Rect,
        live_status: &LiveStatusViewModel,
    ) -> Option<(usize, CharIdx, CharIdx)> {
        let (logic_idx, abs_char_idx) = self.screen_to_anchor(row, col, rect, live_status)?;
        let line = self.get_line_content(logic_idx, live_status)?;
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

    /// Return selected text for the supplied selection state and document.
    pub fn selected_text_for_view(
        &self,
        view: &crate::tui::view_state::output::OutputViewState,
        live_status: &LiveStatusViewModel,
    ) -> Option<String> {
        let (start, end) = view.selection_range()?;
        self.selected_text_for_range(start, end, live_status)
    }

    fn selected_text_for_range(
        &self,
        start: crate::tui::view_state::output::SelectionAnchor,
        end: crate::tui::view_state::output::SelectionAnchor,
        live_status: &LiveStatusViewModel,
    ) -> Option<String> {
        let (start_logic, start_col) = start;
        let (end_logic, end_col) = end;

        if start_logic == end_logic && start_col == end_col {
            return None;
        }

        let total = self
            .document
            .total_lines()
            .max(self.total_virtual_line_count(live_status));
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

            let Some(content) = self.get_line_content(logic_idx, live_status) else {
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
}

pub(crate) fn output_selection_view_for_test(
    start: (usize, CharIdx),
    end: (usize, CharIdx),
) -> OutputViewState {
    let mut view = OutputViewState::default();
    view.begin_selection(start.0, start.1);
    view.update_selection(end.0, end.1);
    view.end_selection();
    view
}

#[cfg(test)]
#[path = "../output/selection_tests.rs"]
mod tests;

#[cfg(test)]
mod document_selection_tests {
    use super::super::OutputArea;
    use crate::tui::render::output::rendered::{RenderedBlock, RenderedDocument, RenderedLine};
    use crate::tui::render::output_area::selection::output_selection_view_for_test;
    use crate::tui::view_model::LiveStatusViewModel;
    use ratatui::text::Span;
    use sdk::CharIdx;
    #[test]
    fn test_copy_selection_returns_plain_chars_across_lines() {
        let mut area = OutputArea::new();
        area.replace_document(RenderedDocument {
            blocks: vec![RenderedBlock {
                block_id: "a".into(),
                lines: vec![
                    RenderedLine::with_plain(vec![Span::raw("**bold**")], "bold".into()),
                    RenderedLine::new(vec![Span::raw("世界")]),
                ],
            }],
        });
        let view = output_selection_view_for_test((0, CharIdx::new(0)), (1, CharIdx::new(2)));
        let copied = area.selected_text_for_view(&view, &LiveStatusViewModel::default());

        assert_eq!(copied.as_deref(), Some("bold\n世界"));
    }
}
