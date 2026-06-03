use super::InputArea;
use crate::tui::render::display::safe_text::{col_to_char_idx, safe_char_slice};
use ratatui::layout::Rect;

pub fn text_anchor_for_screen_col(text: &str, row: usize, screen_col: usize) -> (usize, usize) {
    let char_col = text
        .split('\n')
        .nth(row)
        .map(|line| col_to_char_idx(line, screen_col))
        .unwrap_or(0);
    (row, char_col)
}

pub fn selected_text_for_range_in_text(
    text: &str,
    (start_row, start_col): (usize, usize),
    (end_row, end_col): (usize, usize),
) -> Option<String> {
    let lines: Vec<&str> = text.split('\n').collect();
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

impl InputArea {
    /// 屏幕坐标 → input text `(row, col)` 锚点只读折算。
    pub fn screen_to_input_anchor(
        &self,
        text: &str,
        row: u16,
        col: u16,
        inner_area: &Rect,
    ) -> (usize, usize) {
        let text_row = row.saturating_sub(inner_area.y) as usize;
        let screen_col = col.saturating_sub(inner_area.x) as usize;
        text_anchor_for_screen_col(text, text_row, screen_col)
    }

    /// 获取选中的文本。
    ///
    /// 生产路径必须传入 `InputSelectionViewState` 与 input document text，避免读取 widget 镜像。
    pub fn selected_text_for_view(
        &self,
        text: &str,
        view: &crate::tui::view_state::InputSelectionViewState,
    ) -> Option<String> {
        let ((start_row, start_col), (end_row, end_col)) = view.normalized_selection()?;
        selected_text_for_range_in_text(text, (start_row, start_col), (end_row, end_col))
    }

    /// 获取 widget 镜像选区对应的文本。仅用于选区镜像测试；文本来自调用方。
    #[cfg(test)]
    pub fn get_selected_text_for_text(&self, text: &str) -> Option<String> {
        let ((start_row, start_col), (end_row, end_col)) = self.get_normalized_selection()?;
        selected_text_for_range_in_text(text, (start_row, start_col), (end_row, end_col))
    }

    /// 清除选中。
    #[cfg(test)]
    pub fn clear_selection(&mut self) {
        self.selection_start = None;
        self.selection_end = None;
        self.is_selecting = false;
    }

    /// 由 adapter（`apply_input_selection_to_widget`）单向写回 input 选区镜像。
    ///
    /// #59 S4：选区真相在 `view_state::InputSelectionViewState`，widget 的
    /// `is_selecting`/`selection_start`/`selection_end` 降为只读镜像，供 render 期
    /// 高亮。这是这些镜像字段的**唯一**生产写入路径（widget 内部 `clear_selection`
    /// 与测试除外）。
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
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 屏幕坐标经只读折算 + 镜像写回（adapter 唯一生产写入路径），驱动 plain 取文本。
    fn select_via_mirror(
        input: &mut InputArea,
        text: &str,
        sr: u16,
        sc: u16,
        er: u16,
        ec: u16,
        inner: &Rect,
    ) {
        let start = input.screen_to_input_anchor(text, sr, sc, inner);
        let end = input.screen_to_input_anchor(text, er, ec, inner);
        input.apply_selection_mirror(true, Some(start), Some(end));
    }

    #[test]
    fn test_start_selection_maps_cjk_screen_col_to_char_index() {
        let mut input = InputArea::new();
        let text = "你好a";
        let inner = Rect {
            x: 10,
            y: 5,
            width: 20,
            height: 3,
        };

        select_via_mirror(&mut input, text, 5, 12, 5, 15, &inner);

        assert_eq!(
            input.get_selected_text_for_text(text),
            Some("好a".to_string())
        );
    }

    #[test]
    fn test_start_selection_maps_emoji_screen_col_to_char_index() {
        let mut input = InputArea::new();
        let text = "a🚀b";
        let inner = Rect {
            x: 10,
            y: 5,
            width: 20,
            height: 3,
        };

        select_via_mirror(&mut input, text, 5, 11, 5, 14, &inner);

        assert_eq!(
            input.get_selected_text_for_text(text),
            Some("🚀b".to_string())
        );
    }

    #[test]
    fn test_screen_to_input_anchor_maps_screen_col_without_mutating_state() {
        let input = InputArea::new();
        let text = "你好a";
        let inner = Rect {
            x: 10,
            y: 5,
            width: 20,
            height: 3,
        };

        assert_eq!(input.screen_to_input_anchor(text, 5, 12, &inner), (0, 1));
        assert_eq!(input.screen_to_input_anchor(text, 5, 99, &inner), (0, 3));
        assert_eq!(input.screen_to_input_anchor(text, 8, 12, &inner), (3, 0));
        assert!(!input.is_selecting());
        assert_eq!(input.get_selected_text_for_text(text), None);
    }

    #[test]
    fn test_start_selection_boundary_end_col_clamps_to_line_len() {
        let mut input = InputArea::new();
        let text = "你好";
        let inner = Rect {
            x: 10,
            y: 5,
            width: 20,
            height: 3,
        };

        select_via_mirror(&mut input, text, 5, 10, 5, 99, &inner);

        assert_eq!(
            input.get_selected_text_for_text(text),
            Some("你好".to_string())
        );
    }
}
