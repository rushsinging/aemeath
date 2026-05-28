#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct InputDocument {
    pub buffer: String,
    pub cursor: usize,
    pub selection: Option<InputSelection>,
}

impl InputDocument {
    pub fn insert_text(&mut self, text: &str) {
        let cursor = self.cursor.min(self.buffer.len());
        self.buffer.insert_str(cursor, text);
        self.cursor = cursor + text.len();
        self.selection = None;
    }

    pub fn move_cursor(&mut self, cursor: usize) {
        self.cursor = cursor.min(self.buffer.len());
        self.selection = None;
    }

    pub fn delete_backward(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let remove_at = self.cursor - 1;
        self.buffer.remove(remove_at);
        self.cursor = remove_at;
        self.selection = None;
    }

    pub fn clear(&mut self) {
        self.buffer.clear();
        self.cursor = 0;
        self.selection = None;
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InputSelection {
    pub start: usize,
    pub end: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_insert_text_advances_cursor() {
        let mut doc = InputDocument::default();
        doc.insert_text("abc");
        assert_eq!(doc.buffer, "abc");
        assert_eq!(doc.cursor, 3);
    }

    #[test]
    fn test_move_cursor_clamps_to_buffer() {
        let mut doc = InputDocument::default();
        doc.insert_text("abc");
        doc.move_cursor(99);
        assert_eq!(doc.cursor, 3);
        doc.move_cursor(0);
        assert_eq!(doc.cursor, 0);
    }

    #[test]
    fn test_delete_backward_at_start_is_noop() {
        let mut doc = InputDocument::default();
        doc.delete_backward();
        assert_eq!(doc.buffer, "");
        assert_eq!(doc.cursor, 0);
    }
}
