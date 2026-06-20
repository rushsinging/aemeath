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
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn test_app() -> App {
        App::new(
            "test-session".to_string(),
            PathBuf::from("/tmp"),
            "test-model".to_string(),
        )
    }

    #[test]
    fn records_terminal_size() {
        let mut app = test_app();

        app.handle_resize(80, 24);

        assert_eq!(
            app.layout.last_terminal_size,
            Some(TerminalSize {
                width: 80,
                height: 24,
            })
        );
    }

    #[test]
    fn handle_resize_leaves_input_width_as_render_projection() {
        let mut app = test_app();

        app.handle_resize(80, 24);

        assert_eq!(crate::tui::InputArea::input_content_width(80), 78);
    }

    #[test]
    fn handle_resize_updates_output_view_state_visible_height() {
        let mut app = test_app();

        app.handle_resize(80, 24);

        assert_eq!(app.view_state.output.last_visible_height, 17);
        assert_eq!(app.view_state.output.last_document_total_lines, 0);
        assert!(app.view_state.output.auto_scroll);
    }

    #[test]
    fn test_handle_resize_marks_output_dirty() {
        // 回归（#425 A1 关联）：resize 改变渲染宽度，document 必须按新宽度重 wrap。
        // 重 wrap 只能由 refresh_output_document_from_model 触发，故 resize MUST 标脏 output。
        // A1 前 idle resize 搭 SpinnerTick 每 90ms 标脏的便车；A1 后 idle 不再标脏，
        // 若 handle_resize 不显式标脏，idle resize 将不重 wrap（显示错乱）。
        let mut app = test_app();
        app.view_state.dirty.clear_output();
        app.handle_resize(100, 30);
        assert!(
            app.view_state.dirty.output,
            "resize 改变宽度必须标脏 output，不能依赖 SpinnerTick 便车"
        );
    }

    #[test]
    fn handle_resize_clears_view_state_selection_truth() {
        use crate::tui::render::status::StatusBarRow;
        use sdk::CharIdx;
        let mut app = test_app();
        // 在 view_state（三区选区真相）中建立选区。
        app.view_state.output.begin_selection(1, CharIdx::new(2));
        app.view_state.output.update_selection(3, CharIdx::new(7));
        assert!(app.view_state.output.selection_range().is_some());
        app.view_state
            .status_sel
            .begin_selection(StatusBarRow::Runtime, 2, 80);
        app.view_state.status_sel.update_selection(6);
        assert!(app.view_state.status_sel.selection_range().is_some());
        app.view_state.input_sel.begin_selection((0, 2));
        app.view_state.input_sel.update_selection((0, 6));
        assert!(app.view_state.input_sel.normalized_selection().is_some());

        // 触发 resize（用与初始不同的尺寸，避免 early-return）。
        app.handle_resize(100, 30);

        // 三区真相被清空。
        assert_eq!(app.view_state.output.selection_range(), None);
        assert!(!app.view_state.output.is_selecting());
        assert_eq!(app.view_state.status_sel.selection_range(), None);
        assert!(!app.view_state.status_sel.is_selecting());
        assert_eq!(app.view_state.input_sel.normalized_selection(), None);
        assert!(!app.view_state.input_sel.is_selecting());
    }

    #[test]
    fn duplicate_resize_does_not_process_again() {
        let mut app = test_app();
        app.handle_resize(80, 24);
        app.output_area.set_plain_document_lines(20);
        app.view_state.output.last_visible_height = 12;
        app.view_state.output.last_document_total_lines = 20;
        app.view_state.output.scroll_offset = 7;
        app.view_state.output.auto_scroll = false;
        app.output_area.term_width = 7;

        app.handle_resize(80, 24);

        assert_eq!(app.view_state.output.scroll_offset, 7);
        assert!(!app.view_state.output.auto_scroll);
        assert_eq!(app.output_area.term_width, 7);
        assert_eq!(
            app.layout.last_terminal_size,
            Some(TerminalSize {
                width: 80,
                height: 24,
            })
        );
    }
}
