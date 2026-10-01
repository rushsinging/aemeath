use super::InputArea;
#[cfg(test)]
use crate::tui::render::display::safe_text::col_to_char_idx;
use crate::tui::render::display::safe_text::safe_char_slice;
use crate::tui::render::input::input_area::wrap::{
    anchor_for_display_position, wrap_input_lines_for_width,
};
use ratatui::layout::Rect;

#[cfg(test)]
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
#[path = "selection_tests.rs"]
mod tests;
