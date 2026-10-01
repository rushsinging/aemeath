use super::*;

#[test]
fn test_screen_col_to_char_idx_regression() {
    assert_eq!(screen_col_to_char_idx("a🚀b", 0), CharIdx::new(0));
    assert_eq!(screen_col_to_char_idx("a🚀b", 1), CharIdx::new(1));
    assert_eq!(screen_col_to_char_idx("a🚀b", 3), CharIdx::new(2));
}
