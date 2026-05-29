use super::InputArea;
use crate::tui::render::display::safe_text::{col_to_char_idx, safe_char_slice};
use ratatui::layout::Rect;

impl InputArea {
    /// 开始选中。row/col 是相对于 input_area inner rect 的偏移
    pub fn start_selection(&mut self, row: u16, col: u16, inner_area: &Rect) {
        let pos = self.textarea_pos(row, col, inner_area);
        self.selection_start = Some(pos);
        self.selection_end = Some(pos);
        self.is_selecting = true;
    }

    /// 屏幕坐标 → textarea `(row, col)` 锚点只读折算（#59 S4 T4）。
    ///
    /// 仿 output `screen_to_anchor` / status `screen_to_status_anchor`：依赖 render 期
    /// `textarea.lines()` 布局，把相对 inner rect 的屏幕 (row, col) 折算成 textarea
    /// `(行号, 字符索引)` 锚点，**不改 widget 选区字段**。供 mouse_handler 写入
    /// `view_state.input_sel`（选区真相）。折算逻辑与 `start_selection` 内的
    /// `textarea_pos` 一致，无行为漂移。
    pub fn screen_to_input_anchor(&self, row: u16, col: u16, inner_area: &Rect) -> (usize, usize) {
        self.textarea_pos(row, col, inner_area)
    }

    /// 更新选中位置
    pub fn update_selection(&mut self, row: u16, col: u16, inner_area: &Rect) {
        if !self.is_selecting {
            return;
        }
        self.selection_end = Some(self.textarea_pos(row, col, inner_area));
    }

    /// 结束选中并返回选中文本，不在 selection 层执行剪贴板副作用
    pub fn end_selection(&mut self) -> Option<String> {
        self.is_selecting = false;
        let text = self.get_selected_text();
        self.selection_start = None;
        self.selection_end = None;
        text
    }

    /// 获取选中的文本
    pub fn get_selected_text(&self) -> Option<String> {
        let ((start_row, start_col), (end_row, end_col)) = self.get_normalized_selection()?;
        let lines = self.textarea.lines();
        let mut result = String::new();

        for row in start_row..=end_row {
            if row >= lines.len() {
                break;
            }
            let line_chars: Vec<char> = lines[row].chars().collect();
            let from = if row == start_row { start_col } else { 0 };
            let to = if row == end_row {
                end_col
            } else {
                line_chars.len()
            };
            if row > start_row {
                result.push('\n');
            }
            result.extend(safe_char_slice(&line_chars, from, to).iter());
        }

        if result.is_empty() {
            None
        } else {
            Some(result)
        }
    }

    /// 清除选中
    pub fn clear_selection(&mut self) {
        self.selection_start = None;
        self.selection_end = None;
        self.is_selecting = false;
    }

    /// 由 adapter（`apply_input_selection_to_widget`）单向写回 input 选区镜像。
    ///
    /// #59 S4：选区真相在 `view_state::InputSelectionViewState`，widget 的
    /// `is_selecting`/`selection_start`/`selection_end` 降为只读镜像，供 render 期
    /// 高亮与 `get_selected_text` 取 plain 文本。这是这些镜像字段的**唯一**生产写入
    /// 路径（widget 内部 `clear_selection` 与测试除外）。T4 接线。
    pub(crate) fn apply_selection_mirror(
        &mut self,
        is_selecting: bool,
        selection_start: Option<(usize, usize)>,
        selection_end: Option<(usize, usize)>,
    ) {
        self.is_selecting = is_selecting;
        self.selection_start = selection_start;
        self.selection_end = selection_end;
    }

    /// 是否正在选中
    pub fn is_selecting(&self) -> bool {
        self.is_selecting
    }

    /// 获取归一化的选中范围 (start <= end)
    pub(super) fn get_normalized_selection(&self) -> Option<((usize, usize), (usize, usize))> {
        let start = self.selection_start?;
        let end = self.selection_end?;
        if start == end {
            return None;
        }
        if start.0 < end.0 || (start.0 == end.0 && start.1 < end.1) {
            Some((start, end))
        } else {
            Some((end, start))
        }
    }
    fn textarea_pos(&self, row: u16, col: u16, inner_area: &Rect) -> (usize, usize) {
        let text_row = row.saturating_sub(inner_area.y) as usize;
        let screen_col = col.saturating_sub(inner_area.x) as usize;
        let char_col = self
            .textarea
            .lines()
            .get(text_row)
            .map(|line| col_to_char_idx(line, screen_col))
            .unwrap_or(0);
        (text_row, char_col)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_start_selection_maps_cjk_screen_col_to_char_index() {
        let mut input = InputArea::new();
        input.set_text("你好a");
        let inner = Rect {
            x: 10,
            y: 5,
            width: 20,
            height: 3,
        };

        input.start_selection(5, 12, &inner);
        input.update_selection(5, 15, &inner);

        assert_eq!(input.get_selected_text(), Some("好a".to_string()));
    }

    #[test]
    fn test_start_selection_maps_emoji_screen_col_to_char_index() {
        let mut input = InputArea::new();
        input.set_text("a🚀b");
        let inner = Rect {
            x: 10,
            y: 5,
            width: 20,
            height: 3,
        };

        input.start_selection(5, 11, &inner);
        input.update_selection(5, 14, &inner);

        assert_eq!(input.get_selected_text(), Some("🚀b".to_string()));
    }

    #[test]
    fn test_screen_to_input_anchor_maps_screen_col_without_mutating_state() {
        let mut input = InputArea::new();
        input.set_text("你好a");
        let inner = Rect {
            x: 10,
            y: 5,
            width: 20,
            height: 3,
        };

        // 正常路径：CJK 屏幕列折算为字符索引（"好" 起点 = 第 1 字符）。
        assert_eq!(input.screen_to_input_anchor(5, 12, &inner), (0, 1));
        // 边界：超界列钳制到行字符数。
        assert_eq!(input.screen_to_input_anchor(5, 99, &inner), (0, 3));
        // 错误/越界行：不存在的 textarea 行折算为 (row, 0)。
        assert_eq!(input.screen_to_input_anchor(8, 12, &inner), (3, 0));
        // 只读：折算不改 widget 选区状态。
        assert!(!input.is_selecting());
        assert_eq!(input.get_selected_text(), None);
    }

    #[test]
    fn test_start_selection_boundary_end_col_clamps_to_line_len() {
        let mut input = InputArea::new();
        input.set_text("你好");
        let inner = Rect {
            x: 10,
            y: 5,
            width: 20,
            height: 3,
        };

        input.start_selection(5, 10, &inner);
        input.update_selection(5, 99, &inner);

        assert_eq!(input.get_selected_text(), Some("你好".to_string()));
    }
}
