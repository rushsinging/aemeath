impl super::OutputArea {
    pub fn handle_resize(&mut self, width: u16, visible_height_hint: u16) {
        let new_term_width = (width as usize).saturating_sub(2);
        if new_term_width != self.term_width {
            self.term_width = new_term_width;
        }

        // 仅回填可见高度供 view_state 滚动同步使用；滚动钳制真相归 view_state，由每帧
        // 渲染前的 `sync_output_scroll_view_state` 重新钳制。
        self.last_visible_height = visible_height_hint as usize;
    }
}

#[cfg(test)]
pub mod tests {
    use super::super::OutputArea;

    fn output_area_with_term_width(term_width: usize) -> OutputArea {
        let mut output = OutputArea::new();
        output.term_width = term_width;
        output
    }

    #[test]
    fn width_change_updates_term_width() {
        let mut output = output_area_with_term_width(78);

        output.handle_resize(100, 20);

        assert_eq!(output.term_width, 98);
    }

    #[test]
    fn same_width_keeps_term_width() {
        let mut output = output_area_with_term_width(78);

        output.handle_resize(80, 20);

        assert_eq!(output.term_width, 78);
    }

    #[test]
    fn resize_records_visible_height_without_touching_view_state_scroll() {
        let mut output = output_area_with_term_width(78);
        output.set_plain_document_lines(10);

        output.handle_resize(80, 4);

        assert_eq!(output.last_visible_height, 4);
    }
}
