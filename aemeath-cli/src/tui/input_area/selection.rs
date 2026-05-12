use super::InputArea;
use crate::tui::safe_text::safe_char_slice;
use ratatui::layout::Rect;

impl InputArea {
    /// 开始选中。row/col 是相对于 input_area inner rect 的偏移
    pub fn start_selection(&mut self, row: u16, col: u16, inner_area: &Rect) {
        let pos = textarea_pos(row, col, inner_area);
        self.selection_start = Some(pos);
        self.selection_end = Some(pos);
        self.is_selecting = true;
    }

    /// 更新选中位置
    pub fn update_selection(&mut self, row: u16, col: u16, inner_area: &Rect) {
        if !self.is_selecting {
            return;
        }
        self.selection_end = Some(textarea_pos(row, col, inner_area));
    }

    /// 结束选中并复制到剪贴板
    pub fn end_selection(&mut self) -> Option<String> {
        self.is_selecting = false;
        let text = self.get_selected_text();
        if let Some(ref t) = text {
            copy_to_clipboard(t);
        }
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

fn textarea_pos(row: u16, col: u16, inner_area: &Rect) -> (usize, usize) {
    (
        row.saturating_sub(inner_area.y) as usize,
        col.saturating_sub(inner_area.x) as usize,
    )
}

fn copy_to_clipboard(text: &str) {
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
