use super::App;
use crate::tui::app::state::TerminalSize;

impl App {
    pub(crate) fn handle_resize(&mut self, width: u16, height: u16) {
        let size = TerminalSize { width, height };
        if self.layout.last_terminal_size == Some(size) {
            return;
        }

        self.layout.last_terminal_size = Some(size);
        self.output_area.handle_resize(width);
        let visible_height = crate::tui::app::update::output_visible_height(
            self.layout
                .output_area_rect
                .height
                .max(height.saturating_sub(7)),
            &self.live_status_view_model(),
        );
        self.view_state
            .output
            .sync_document_metrics(self.output_area.document().total_lines(), visible_height);
        // 选区真相归 view_state：resize 时清三区选区真相，否则 widget 的镜像清空会被下一帧
        // adapter 用旧 view_state 选区复活（resize 仅作用于 widget 镜像，不动真相）。
        self.view_state.output.clear_selection();
        self.view_state.status_sel.clear_selection();
        self.view_state.input_sel.clear_selection();
        // resize 改变渲染宽度 → document 必须按新宽度重 wrap（仅 refresh 能做）。
        // 显式标脏 output，不再依赖 SpinnerTick 每帧标脏的便车（A1 后 idle 不再标脏）。
        self.mark_output_dirty();
    }
}

#[cfg(test)]
#[path = "resize_tests.rs"]
mod tests;
