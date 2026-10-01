use sdk::CharIdx;

use crate::tui::render::display::safe_text;

/// Convert a screen column position (display column) to a char index within the string.
pub fn screen_col_to_char_idx(text: &str, screen_col: usize) -> CharIdx {
    CharIdx::new(safe_text::col_to_char_idx(text, screen_col))
}

#[cfg(test)]
#[path = "display_tests.rs"]
mod tests;
