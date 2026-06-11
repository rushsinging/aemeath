use super::InputArea;
use crate::tui::render::display::safe_text::{col_to_char_idx, safe_char_slice};
use crate::tui::render::input::input_area::wrap::{
    anchor_for_display_position, wrap_input_lines_for_width,
};
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
        let display_row = row.saturating_sub(inner_area.y) as usize;
        let screen_col = col.saturating_sub(inner_area.x) as usize;
        let width = inner_area.width as usize;
        let display_lines = wrap_input_lines_for_width(text.split('\n').collect(), width);
        anchor_for_display_position(&display_lines, display_row, screen_col)
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::view_state::InputSelectionViewState;

    fn selection_view(start: (usize, usize), end: (usize, usize)) -> InputSelectionViewState {
        let mut view = InputSelectionViewState::default();
        view.begin_selection(start);
        view.update_selection(end);
        view
    }

    #[test]
    fn test_selected_text_for_view_maps_cjk_screen_col_to_char_index() {
        let input = InputArea::new();
        let text = "你好a";
        let inner = Rect {
            x: 10,
            y: 5,
            width: 20,
            height: 3,
        };
        let start = input.screen_to_input_anchor(text, 5, 12, &inner);
        let end = input.screen_to_input_anchor(text, 5, 15, &inner);
        let view = selection_view(start, end);

        assert_eq!(
            input.selected_text_for_view(text, &view),
            Some("好a".to_string())
        );
    }

    #[test]
    fn test_selected_text_for_view_maps_emoji_screen_col_to_char_index() {
        let input = InputArea::new();
        let text = "a🚀b";
        let inner = Rect {
            x: 10,
            y: 5,
            width: 20,
            height: 3,
        };
        let start = input.screen_to_input_anchor(text, 5, 11, &inner);
        let end = input.screen_to_input_anchor(text, 5, 14, &inner);
        let view = selection_view(start, end);

        assert_eq!(
            input.selected_text_for_view(text, &view),
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
    }

    #[test]
    fn test_screen_to_input_anchor_maps_wrapped_display_row_to_original_anchor() {
        let input = InputArea::new();
        let text = "abcdef";
        let inner = Rect {
            x: 10,
            y: 5,
            width: 4,
            height: 3,
        };

        assert_eq!(input.screen_to_input_anchor(text, 6, 11, &inner), (0, 5));
    }

    #[test]
    fn test_selected_text_for_view_boundary_end_col_clamps_to_line_len() {
        let input = InputArea::new();
        let text = "你好";
        let inner = Rect {
            x: 10,
            y: 5,
            width: 20,
            height: 3,
        };
        let start = input.screen_to_input_anchor(text, 5, 10, &inner);
        let end = input.screen_to_input_anchor(text, 5, 99, &inner);
        let view = selection_view(start, end);

        assert_eq!(
            input.selected_text_for_view(text, &view),
            Some("你好".to_string())
        );
    }
}
