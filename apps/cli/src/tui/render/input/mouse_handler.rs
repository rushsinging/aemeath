use crate::tui::render::status::StatusBarRow;
use crossterm::event::{MouseEvent, MouseEventKind};
use std::time::Instant;

/// 判断点 (row, col) 是否在 rect 内
fn point_in_rect(row: u16, col: u16, rect: &ratatui::layout::Rect) -> bool {
    row >= rect.y && row < rect.y + rect.height && col >= rect.x && col < rect.x + rect.width
}

impl crate::tui::app::App {
    /// 处理鼠标事件。复制选区等副作用以 Effect 形式返回，由 update/runtime 执行。
    pub(crate) fn handle_mouse_event(
        &mut self,
        mouse: MouseEvent,
        output_area: ratatui::layout::Rect,
    ) -> Vec<crate::tui::effect::effect::Effect> {
        let row = mouse.row;
        let col = mouse.column;

        match mouse.kind {
            MouseEventKind::ScrollUp => {
                if point_in_rect(row, col, &output_area) {
                    // 滚动真相归 view_state；widget 镜像每帧由 adapter 写回。
                    let total_lines = self.output_area.document().total_lines();
                    self.view_state.output.scroll_up(3, total_lines);
                }
                return Vec::new();
            }
            MouseEventKind::ScrollDown => {
                if point_in_rect(row, col, &output_area) {
                    self.view_state.output.scroll_down(3);
                }
                return Vec::new();
            }
            _ => {}
        }

        // 计算各区域 rect
        let input_area = self.layout.input_area_rect;
        let status_bar = self.layout.status_bar_rect;

        match mouse.kind {
            MouseEventKind::Down(crossterm::event::MouseButton::Left) => {
                if point_in_rect(row, col, &output_area) {
                    // 双击检测
                    let now = Instant::now();
                    let is_double = self
                        .input
                        .last_click
                        .map(|(t, prev_row, prev_col)| {
                            prev_row == row
                                && prev_col == col
                                && t.elapsed() < std::time::Duration::from_millis(500)
                        })
                        .unwrap_or(false);

                    if is_double {
                        // 输出区选区真相归 view_state；input/status clear 暂留（S4 统一）。
                        self.input_area.clear_selection();
                        self.status_bar.clear_selection();
                        if let Some((line, ws, we)) =
                            self.output_area.word_bounds_at(row, col, &output_area)
                        {
                            self.view_state.output.select_word(line, ws, we);
                        }
                        self.input.last_click = None;
                    } else {
                        // 清除其他区域的选中
                        self.input_area.clear_selection();
                        self.status_bar.clear_selection();
                        if let Some((line, anchor)) =
                            self.output_area.screen_to_anchor(row, col, &output_area)
                        {
                            self.view_state.output.begin_selection(line, anchor);
                        }
                        self.input.last_click = Some((now, row, col));
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
                if self.view_state.output.is_selecting() {
                    // 屏幕坐标 → 锚点折算只读借 widget（依赖 render 期 screen_line_map），
                    // 选区真相写入 view_state；行超界时兜底到末尾锚点。
                    if let Some((line, anchor)) =
                        self.output_area.screen_to_anchor(row, col, &output_area)
                    {
                        self.view_state.output.update_selection(line, anchor);
                    } else if let Some((line, anchor)) = self.output_area.last_visible_anchor() {
                        self.view_state.output.update_selection(line, anchor);
                    }
                } else if self.input_area.is_selecting() {
                    let inner = self.input_area.get_inner_area(&input_area);
                    self.input_area.update_selection(row, col, &inner);
                } else if self.status_bar.is_selecting() {
                    self.status_bar
                        .update_selection_at(col.saturating_sub(status_bar.x), status_bar.width);
                }
            }
            MouseEventKind::Up(crossterm::event::MouseButton::Left) => {
                let text = if self.view_state.output.is_selecting() {
                    // 结束拖拽：view_state 清 is_selecting 但保留锚点（真相）。
                    self.view_state.output.end_selection();
                    // 复制时序关键：取文本前先把 view_state 最新选区同步到 widget 镜像，
                    // 否则 widget 镜像滞后一帧会丢失最后一段选区。
                    crate::tui::adapter::output_view_widget::apply_output_selection_to_widget(
                        &self.view_state.output,
                        &mut self.output_area,
                    );
                    // 取 plain 文本（读 widget 选区镜像 + document，#63 gutter 不进 plain）。
                    let text = self.output_area.get_selected_text();
                    // 取完清选区：view_state 清空，下帧 adapter 同步清 widget 镜像。
                    self.view_state.output.clear_selection();
                    text
                } else if self.input_area.is_selecting() {
                    self.input_area.end_selection()
                } else if self.status_bar.is_selecting() {
                    self.status_bar.end_selection()
                } else {
                    None
                };
                return self.copy_selection_to_clipboard(text).into_iter().collect();
            }
            _ => {}
        }
        Vec::new()
    }
}
