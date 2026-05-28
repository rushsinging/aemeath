use super::InputArea;

impl InputArea {
    pub fn handle_resize(&mut self, width: u16) {
        self.content_width = width.saturating_sub(2);
        if self.is_selecting || self.selection_start.is_some() || self.selection_end.is_some() {
            self.clear_selection();
        }
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    #[test]
    fn resize_updates_content_width() {
        let mut input = InputArea::new();

        input.handle_resize(80);

        assert_eq!(input.content_width, 78);
    }

    #[test]
    fn small_width_saturates_content_width() {
        let mut input = InputArea::new();

        input.handle_resize(1);

        assert_eq!(input.content_width, 0);
    }

    #[test]
    fn resize_clears_active_selection() {
        let mut input = InputArea::new();
        input.is_selecting = true;
        input.selection_start = Some((0, 0));
        input.selection_end = Some((0, 4));

        input.handle_resize(80);

        assert!(!input.is_selecting);
        assert!(input.selection_start.is_none());
        assert!(input.selection_end.is_none());
    }
}
