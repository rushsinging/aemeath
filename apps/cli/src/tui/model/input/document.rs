#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct InputDocument {
    pub buffer: String,
    pub cursor: usize,
    pub selection: Option<InputSelection>,
}

impl InputDocument {
    pub fn insert_text(&mut self, text: &str) {
        let cursor = self.cursor.min(self.buffer.len());
        let cursor = clamp_to_char_boundary(&self.buffer, cursor);
        self.buffer.insert_str(cursor, text);
        self.cursor = cursor + text.len();
        self.selection = None;
    }

    pub fn move_cursor(&mut self, cursor: usize) {
        self.cursor = clamp_to_char_boundary(&self.buffer, cursor.min(self.buffer.len()));
        self.selection = None;
    }

    pub fn move_left(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let mut previous = 0;
        for (idx, _) in self.buffer.char_indices() {
            if idx >= self.cursor {
                break;
            }
            previous = idx;
        }
        self.cursor = previous;
        self.selection = None;
    }

    pub fn move_right(&mut self) {
        if self.cursor >= self.buffer.len() {
            return;
        }
        let next = self.buffer[self.cursor..]
            .chars()
            .next()
            .map(|ch| self.cursor + ch.len_utf8())
            .unwrap_or(self.buffer.len());
        self.cursor = next.min(self.buffer.len());
        self.selection = None;
    }

    pub fn move_home(&mut self) {
        self.cursor = 0;
        self.selection = None;
    }

    pub fn move_end(&mut self) {
        self.cursor = self.buffer.len();
        self.selection = None;
    }

    pub fn delete_backward(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let old_cursor = self.cursor;
        self.move_left();
        self.buffer.drain(self.cursor..old_cursor);
        self.selection = None;
    }

    pub fn delete_forward(&mut self) {
        if self.cursor >= self.buffer.len() {
            return;
        }
        let start = self.cursor;
        self.move_right();
        let end = self.cursor;
        self.buffer.drain(start..end);
        self.cursor = start;
        self.selection = None;
    }

    pub fn clear(&mut self) {
        self.buffer.clear();
        self.cursor = 0;
        self.selection = None;
    }
}

fn clamp_to_char_boundary(text: &str, cursor: usize) -> usize {
    let mut cursor = cursor.min(text.len());
    while cursor > 0 && !text.is_char_boundary(cursor) {
        cursor -= 1;
    }
    cursor
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
