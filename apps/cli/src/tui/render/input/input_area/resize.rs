use super::InputArea;

impl InputArea {
    pub fn handle_resize(&mut self, width: u16) {
        self.content_width = width.saturating_sub(2);
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
}
