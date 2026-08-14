use super::*;

#[test]
fn test_slice_head_ascii_and_short() {
    assert_eq!(slice_head("hello", 3), "hel");
    assert_eq!(slice_head("hi", 10), "hi");
}

#[test]
fn test_slice_head_cjk_rounds_down() {
    assert_eq!(slice_head("你好世界", 4), "你");
    assert_eq!(slice_head("你好世界", 6), "你好");
}

#[test]
fn test_slice_tail_preserves_ascii_tail() {
    assert_eq!(slice_tail("abcdef", 3), "def");
}

#[test]
fn test_slice_tail_keeps_full_string_when_under_limit() {
    assert_eq!(slice_tail("hi", 10), "hi");
}

#[test]
fn test_slice_tail_aligns_to_utf8_boundary() {
    assert_eq!(slice_tail("你好世界", 4), "界");
    assert_eq!(slice_tail("你好世界", 6), "世界");
}

#[test]
fn test_slice_head_tail_never_panic() {
    let source = "a你好🚀b";
    for max_bytes in 0..=source.len() + 2 {
        let _ = slice_head(source, max_bytes);
        let _ = slice_tail(source, max_bytes);
    }
}
