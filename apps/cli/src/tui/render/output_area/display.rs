use sdk::CharIdx;

use crate::tui::render::display::safe_text;

/// Convert a screen column position (display column) to a char index within the string.
pub fn screen_col_to_char_idx(text: &str, screen_col: usize) -> CharIdx {
    CharIdx::new(safe_text::col_to_char_idx(text, screen_col))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_screen_col_to_char_idx_regression() {
        assert_eq!(screen_col_to_char_idx("a🚀b", 0), CharIdx::new(0));
        assert_eq!(screen_col_to_char_idx("a🚀b", 1), CharIdx::new(1));
        assert_eq!(screen_col_to_char_idx("a🚀b", 3), CharIdx::new(2));
    }
}
