impl super::OutputArea {
    pub fn handle_resize(&mut self, width: u16, visible_height_hint: u16) {
        let new_term_width = (width as usize).saturating_sub(2);
        if new_term_width != self.term_width {
            self.term_width = new_term_width;
        }

        // 仅回填可见高度供 adapter 反喂；滚动钳制真相归 view_state，由每帧渲染前的
        // `apply_output_scroll_to_widget` 重新钳制并覆盖 widget 镜像（resize 与下一次
        // 钳制之间无 draw，故此处就地钳制冗余，已删除）。
        self.last_visible_height = visible_height_hint as usize;

        if self.is_selecting || self.selection_start.is_some() || self.selection_end.is_some() {
            self.clear_selection();
        }
    }
}

#[cfg(test)]
pub mod tests {
    use super::super::OutputArea;
    use sdk::CharIdx;

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
    fn resize_records_visible_height_without_touching_scroll() {
        // 滚动钳制真相归 view_state（由 adapter 每帧重新钳制覆盖镜像）；handle_resize
        // 仅回填 last_visible_height，不再就地改 scroll_offset/auto_scroll。
        let mut output = output_area_with_term_width(78);
        output.set_plain_document_lines(10);
        output.scroll_offset = 9;
        output.auto_scroll = false;

        output.handle_resize(80, 4);

        assert_eq!(output.last_visible_height, 4);
        // 镜像未被就地钳制：保持调用前的值。
        assert_eq!(output.scroll_offset, 9);
        assert!(!output.auto_scroll);
    }

    #[test]
    fn resize_clears_active_selection() {
        let mut output = output_area_with_term_width(78);
        output.is_selecting = true;
        output.selection_start = Some((0, CharIdx::new(0)));
        output.selection_end = Some((0, CharIdx::new(4)));

        output.handle_resize(80, 20);

        assert!(!output.is_selecting);
        assert!(output.selection_start.is_none());
        assert!(output.selection_end.is_none());
    }
}
