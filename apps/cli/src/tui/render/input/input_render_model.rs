use crate::tui::model::input::document::InputDocument;
use crate::tui::render::display::safe_text::safe_byte_prefix;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InputRenderModel {
    pub text: String,
    pub cursor: usize,
    pub cursor_row: usize,
    pub cursor_col: usize,
    pub placeholder: Option<String>,
    pub focused: bool,
}

impl InputRenderModel {
    pub fn from_document(
        document: &InputDocument,
        placeholder: Option<String>,
        focused: bool,
    ) -> Self {
        let cursor = clamp_to_char_boundary(&document.buffer, document.cursor);
        let (cursor_row, cursor_col) = byte_cursor_to_row_col(&document.buffer, cursor);
        Self {
            text: document.buffer.clone(),
            cursor,
            cursor_row,
            cursor_col,
            placeholder,
            focused,
        }
    }

    pub fn lines(&self) -> Vec<&str> {
        self.text.split('\n').collect()
    }
}

pub fn byte_cursor_to_row_col(text: &str, cursor: usize) -> (usize, usize) {
    let cursor = clamp_to_char_boundary(text, cursor);
    let before_cursor = safe_byte_prefix(text, cursor);
    let row = before_cursor.matches('\n').count();
    let col = before_cursor
        .rsplit_once('\n')
        .map(|(_, tail)| tail.chars().count())
        .unwrap_or_else(|| before_cursor.chars().count());
    (row, col)
}

fn clamp_to_char_boundary(text: &str, cursor: usize) -> usize {
    let mut cursor = cursor.min(text.len());
    while cursor > 0 && !text.is_char_boundary(cursor) {
        cursor -= 1;
    }
    cursor
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_byte_cursor_to_row_col_ascii_multiline() {
        assert_eq!(byte_cursor_to_row_col("ab\ncd", 4), (1, 1));
    }

    #[test]
    fn test_byte_cursor_to_row_col_cjk_boundary() {
        let text = "你a\n好b";
        assert_eq!(byte_cursor_to_row_col(text, "你a\n好".len()), (1, 1));
    }

    #[test]
    fn test_byte_cursor_to_row_col_clamps_inside_emoji() {
        let text = "a🚀b";
        assert_eq!(byte_cursor_to_row_col(text, 2), (0, 1));
    }
}
