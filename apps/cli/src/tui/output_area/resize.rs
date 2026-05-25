impl super::OutputArea {
    pub fn handle_resize(&mut self, width: u16, visible_height_hint: u16) {
        let new_term_width = (width as usize).saturating_sub(2);
        if new_term_width != self.term_width {
            self.term_width = new_term_width;
            self.rendered_cache.invalidate();
        }

        let visible_height = visible_height_hint as usize;
        self.last_visible_height = visible_height;

        let max_offset = self.lines.len().saturating_sub(visible_height);
        self.scroll_offset = self.scroll_offset.min(max_offset);
        if self.scroll_offset == 0 {
            self.auto_scroll = true;
        }

        if self.is_selecting || self.selection_start.is_some() || self.selection_end.is_some() {
            self.clear_selection();
        }
    }
}

#[cfg(test)]
pub mod tests {
    use super::super::{LineStyle, OutputArea, OutputLine};
    use kernel::string_idx::CharIdx;

    fn output_area_with_clean_cache(term_width: usize) -> OutputArea {
        let mut output = OutputArea::new();
        output.term_width = term_width;
        output.rendered_cache.mark_clean_for_test(term_width);
        output
    }

    fn push_lines(output: &mut OutputArea, count: usize) {
        for i in 0..count {
            output.push_line(OutputLine {
                content: format!("line {i}"),
                style: LineStyle::Normal,
                ..Default::default()
            });
        }
    }

    #[test]
    fn width_change_invalidates_render_cache() {
        let mut output = output_area_with_clean_cache(78);

        output.handle_resize(100, 20);

        assert_eq!(output.term_width, 98);
        assert!(output.rendered_cache.is_dirty());
    }

    #[test]
    fn same_width_does_not_invalidate_render_cache() {
        let mut output = output_area_with_clean_cache(78);

        output.handle_resize(80, 20);

        assert_eq!(output.term_width, 78);
        assert!(!output.rendered_cache.is_dirty());
    }

    #[test]
    fn resize_clamps_scroll_offset_to_visible_height() {
        let mut output = output_area_with_clean_cache(78);
        push_lines(&mut output, 10);
        output.scroll_offset = 9;
        output.auto_scroll = false;

        output.handle_resize(80, 4);

        assert_eq!(output.last_visible_height, 4);
        assert_eq!(output.scroll_offset, 6);
        assert!(!output.auto_scroll);
    }

    #[test]
    fn resize_restores_auto_scroll_when_offset_is_zero() {
        let mut output = output_area_with_clean_cache(78);
        push_lines(&mut output, 3);
        output.scroll_offset = 2;
        output.auto_scroll = false;

        output.handle_resize(80, 10);

        assert_eq!(output.scroll_offset, 0);
        assert!(output.auto_scroll);
    }

    #[test]
    fn resize_clears_active_selection() {
        let mut output = output_area_with_clean_cache(78);
        output.is_selecting = true;
        output.selection_start = Some((0, CharIdx::new(0)));
        output.selection_end = Some((0, CharIdx::new(4)));

        output.handle_resize(80, 20);

        assert!(!output.is_selecting);
        assert!(output.selection_start.is_none());
        assert!(output.selection_end.is_none());
    }
}
