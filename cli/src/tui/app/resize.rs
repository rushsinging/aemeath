use super::{App, TerminalSize};

impl App {
    pub(crate) fn handle_resize(&mut self, width: u16, height: u16) {
        let size = TerminalSize { width, height };
        if self.last_terminal_size == Some(size) {
            return;
        }

        self.last_terminal_size = Some(size);
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
            app.last_terminal_size,
            Some(TerminalSize {
                width: 80,
                height: 24,
            })
        );
    }

    #[test]
    fn duplicate_resize_does_not_process_again() {
        let mut app = test_app();
        app.handle_resize(80, 24);
        app.output_area.scroll_offset = 7;

        app.handle_resize(80, 24);

        assert_eq!(app.output_area.scroll_offset, 7);
        assert_eq!(
            app.last_terminal_size,
            Some(TerminalSize {
                width: 80,
                height: 24,
            })
        );
    }
}
