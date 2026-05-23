use crate::tui::status_bar::StatusBarRow;
use crossterm::event::{MouseEvent, MouseEventKind};
use std::time::Instant;

/// 判断点 (row, col) 是否在 rect 内
fn point_in_rect(row: u16, col: u16, rect: &ratatui::layout::Rect) -> bool {
    row >= rect.y && row < rect.y + rect.height && col >= rect.x && col < rect.x + rect.width
}

impl super::App {
    pub(super) fn handle_mouse_event(
        &mut self,
        mouse: MouseEvent,
        output_area: ratatui::layout::Rect,
    ) {
        let row = mouse.row;
        let col = mouse.column;

        match mouse.kind {
            MouseEventKind::ScrollUp => {
                if point_in_rect(row, col, &output_area) {
                    self.output_area.scroll_up(3);
                }
                return;
            }
            MouseEventKind::ScrollDown => {
                if point_in_rect(row, col, &output_area) {
                    self.output_area.scroll_down(3);
                }
                return;
            }
            _ => {}
        }

        // 计算各区域 rect
        let input_area = self.input_area_rect;
        let status_bar = self.status_bar_rect;

        match mouse.kind {
            MouseEventKind::Down(crossterm::event::MouseButton::Left) => {
                if point_in_rect(row, col, &output_area) {
                    // 双击检测
                    let now = Instant::now();
                    let is_double = self
                        .last_click
                        .map(|(t, prev_row, prev_col)| {
                            prev_row == row
                                && prev_col == col
                                && t.elapsed() < std::time::Duration::from_millis(500)
                        })
                        .unwrap_or(false);

                    if is_double {
                        self.input_area.clear_selection();
                        self.status_bar.clear_selection();
                        self.output_area.select_word(row, col, &output_area);
                        self.last_click = None;
                    } else {
                        // 清除其他区域的选中
                        self.input_area.clear_selection();
                        self.status_bar.clear_selection();
                        self.output_area.start_selection(row, col, &output_area);
                        self.last_click = Some((now, row, col));
                    }
                } else if point_in_rect(row, col, &input_area) {
                    // 清除其他区域的选中
                    self.output_area.clear_selection();
                    self.status_bar.clear_selection();
                    let inner = self.input_area.get_inner_area(&input_area);
                    self.input_area.start_selection(row, col, &inner);
                } else if point_in_rect(row, col, &status_bar) {
                    // 清除其他区域的选中
                    self.output_area.clear_selection();
                    self.input_area.clear_selection();
                    let status_row = if row == status_bar.y.saturating_add(1) {
                        StatusBarRow::Context
                    } else {
                        StatusBarRow::Runtime
                    };
                    self.status_bar.start_selection_at(
                        status_row,
                        col.saturating_sub(status_bar.x),
                        status_bar.width,
                    );
                }
            }
            MouseEventKind::Drag(crossterm::event::MouseButton::Left) => {
                if self.output_area.is_selecting() {
                    self.output_area.update_selection(row, col, &output_area);
                } else if self.input_area.is_selecting() {
                    let inner = self.input_area.get_inner_area(&input_area);
                    self.input_area.update_selection(row, col, &inner);
                } else if self.status_bar.is_selecting() {
                    self.status_bar
                        .update_selection_at(col.saturating_sub(status_bar.x), status_bar.width);
                }
            }
            MouseEventKind::Up(crossterm::event::MouseButton::Left) => {
                if self.output_area.is_selecting() {
                    let text = self.output_area.end_selection();
                    self.copy_selection_to_clipboard(text);
                } else if self.input_area.is_selecting() {
                    let text = self.input_area.end_selection();
                    self.copy_selection_to_clipboard(text);
                } else if self.status_bar.is_selecting() {
                    let text = self.status_bar.end_selection();
                    self.copy_selection_to_clipboard(text);
                }
            }
            _ => {}
        }
    }
}
