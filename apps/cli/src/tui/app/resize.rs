use super::App;
use crate::tui::app::state::TerminalSize;

impl App {
    pub(crate) fn handle_resize(&mut self, width: u16, height: u16) {
        let size = TerminalSize { width, height };
        if self.layout.last_terminal_size == Some(size) {
            return;
        }

        self.layout.last_terminal_size = Some(size);
        let visible_height_hint = self
            .layout
            .output_area_rect
            .height
            .max(height.saturating_sub(7));
        self.output_area.handle_resize(width, visible_height_hint);
        // 选区真相归 view_state：resize 时清三区选区真相，否则 widget 的镜像清空会被下一帧
        // adapter 用旧 view_state 选区复活（resize 仅作用于 widget 镜像，不动真相）。
        self.view_state.output.clear_selection();
        self.view_state.status_sel.clear_selection();
        self.view_state.input_sel.clear_selection();
        let input_width = self.layout.input_area_rect.width.max(width);
        self.input_area.handle_resize(input_width);
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
    fn handle_resize_updates_input_content_width() {
        let mut app = test_app();

        app.handle_resize(80, 24);

        assert_eq!(app.input_area.content_width, 78);
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

        // 三区真相被清空：避免下一帧 adapter 复活 widget 镜像。
        assert_eq!(app.view_state.output.selection_range(), None);
        assert!(!app.view_state.output.is_selecting());
        assert_eq!(app.view_state.status_sel.selection_range(), None);
        assert!(!app.view_state.status_sel.is_selecting());
        assert_eq!(app.view_state.input_sel.normalized_selection(), None);
        assert!(!app.view_state.input_sel.is_selecting());

        // 经渲染前刷新后，widget 镜像也被同步清空。
        app.refresh_output_scroll_from_view_state();
        assert!(!app.output_area.is_selecting);
        assert!(app.output_area.selection_start.is_none());
        assert!(app.output_area.selection_end.is_none());
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
        app.refresh_output_scroll_from_view_state();
        app.output_area.term_width = 7;

        app.handle_resize(80, 24);

        assert_eq!(app.output_area.scroll_offset, 3);
        assert!(!app.output_area.auto_scroll);
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
