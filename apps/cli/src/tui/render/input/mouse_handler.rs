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
                        // 输出区选区真相归 view_state；status/input 清 view_state（S4）。
                        self.view_state.input_sel.clear_selection();
                        self.view_state.status_sel.clear_selection();
                        if let Some((line, ws, we)) =
                            self.output_area.word_bounds_at(row, col, &output_area)
                        {
                            self.view_state.output.select_word(line, ws, we);
                        }
                        self.input.last_click = None;
                    } else {
                        // 清除其他区域的选中（status/input 清 view_state）
                        self.view_state.input_sel.clear_selection();
                        self.view_state.status_sel.clear_selection();
                        if let Some((line, anchor)) =
                            self.output_area.screen_to_anchor(row, col, &output_area)
                        {
                            self.view_state.output.begin_selection(line, anchor);
                        }
                        self.input.last_click = Some((now, row, col));
                    }
                } else if point_in_rect(row, col, &input_area) {
                    // 清除其他区域的选中（output/status 清 view_state）
                    self.view_state.output.clear_selection();
                    self.view_state.status_sel.clear_selection();
                    // 屏幕坐标 → textarea 锚点只读折算借 widget（依赖 render 期布局），
                    // 选区真相写入 view_state（#59 S4 T4）。
                    let inner = self.input_area.get_inner_area(&input_area);
                    let anchor = self.input_area.screen_to_input_anchor(
                        &self.model.input.document.buffer,
                        row,
                        col,
                        &inner,
                    );
                    self.view_state.input_sel.begin_selection(anchor);
                } else if point_in_rect(row, col, &status_bar) {
                    // 清除其他区域的选中（output/input 清 view_state）
                    self.view_state.output.clear_selection();
                    self.view_state.input_sel.clear_selection();
                    // 屏幕坐标 → status 锚点只读折算借 widget（依赖 render 期布局），
                    // 选区真相写入 view_state（#59 S4）。
                    let (status_row, char_idx, width) = self.status_bar.screen_to_status_anchor(
                        row,
                        col,
                        status_bar.y,
                        status_bar.x,
                        status_bar.width,
                    );
                    self.view_state
                        .status_sel
                        .begin_selection(status_row, char_idx, width);
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
                } else if self.view_state.input_sel.is_selecting() {
                    // 屏幕坐标 → textarea 锚点只读折算借 widget，写入 view_state。
                    let inner = self.input_area.get_inner_area(&input_area);
                    let anchor = self.input_area.screen_to_input_anchor(
                        &self.model.input.document.buffer,
                        row,
                        col,
                        &inner,
                    );
                    self.view_state.input_sel.update_selection(anchor);
                } else if self.view_state.status_sel.is_selecting() {
                    // 据 view_state 已记录的 row/width 折算拖拽列 → char_idx，写入 view_state。
                    let sel = &self.view_state.status_sel;
                    let char_idx = self.status_bar.screen_col_to_char_idx(
                        sel.selection_row,
                        col.saturating_sub(status_bar.x),
                        sel.selection_width,
                    );
                    self.view_state.status_sel.update_selection(char_idx);
                }
            }
            MouseEventKind::Up(crossterm::event::MouseButton::Left) => {
                let text = if self.view_state.output.is_selecting() {
                    // 结束拖拽：view_state 清 is_selecting 但保留锚点（真相）。
                    self.view_state.output.end_selection();
                    // 取 plain 文本（读 view_state 选区真相 + widget document，#63 gutter 不进 plain）。
                    let text = self
                        .output_area
                        .selected_text_for_view(&self.view_state.output);
                    // 取完清选区：view_state 清空，下帧 adapter 同步清 widget 镜像。
                    self.view_state.output.clear_selection();
                    text
                } else if self.view_state.input_sel.is_selecting() {
                    // 结束拖拽：view_state 清 is_selecting 但保留锚点（真相）。
                    self.view_state.input_sel.end_selection();
                    // 取 plain 文本（读 view_state 选区真相 + render 期 textarea.lines() 折算）。
                    let text = self.input_area.selected_text_for_view(
                        &self.model.input.document.buffer,
                        &self.view_state.input_sel,
                    ); // 取完清选区：view_state 清空，下帧 adapter 同步清 widget 镜像。
                    self.view_state.input_sel.clear_selection();
                    text
                } else if self.view_state.status_sel.is_selecting() {
                    // 结束拖拽：view_state 清 is_selecting 但保留锚点（真相）。
                    self.view_state.status_sel.end_selection();
                    // 取 plain 文本（读 view_state 选区真相 + render 期 line_text 折算）。
                    let text = self
                        .status_bar
                        .selected_text_for_view(&self.view_state.status_sel);
                    // 取完清选区：view_state 清空，下帧 adapter 同步清 widget 镜像。
                    self.view_state.status_sel.clear_selection();
                    text
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
