use super::*;

#[test]
fn test_char_idx_zero() {
    assert_eq!(CharIdx::ZERO.as_usize(), 0);
}

#[test]
fn test_char_idx_new() {
    assert_eq!(CharIdx::new(5).as_usize(), 5);
}

#[test]
fn test_char_idx_count_in_ascii() {
    assert_eq!(CharIdx::count_in("hello").as_usize(), 5);
}

#[test]
fn test_char_idx_count_in_cjk() {
    assert_eq!(CharIdx::count_in("你好世界").as_usize(), 4);
}

#[test]
fn test_char_idx_count_in_emoji() {
    assert_eq!(CharIdx::count_in("a🚀b").as_usize(), 3);
}

#[test]
fn test_char_idx_add() {
    let c = CharIdx::new(3);
    assert_eq!(c.advance(5).as_usize(), 8);
}

#[test]
fn test_char_idx_checked_add_within_bounds() {
    let c = CharIdx::new(2);
    assert_eq!(c.checked_add(3, "hello").unwrap().as_usize(), 5);
}

#[test]
fn test_char_idx_checked_add_out_of_bounds() {
    let c = CharIdx::new(3);
    assert!(c.checked_add(3, "hello").is_none());
}

#[test]
fn test_char_idx_sub() {
    let a = CharIdx::new(10);
    let b = CharIdx::new(3);
    assert_eq!(a - b, 7);
}

#[test]
fn test_char_idx_saturating_sub() {
    let a = CharIdx::new(3);
    let b = CharIdx::new(10);
    assert_eq!(a.saturating_sub(b), 0);
}
