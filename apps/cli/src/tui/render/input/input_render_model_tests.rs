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
