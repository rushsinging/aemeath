use super::*;

#[test]
fn test_col_idx_zero() {
    assert_eq!(ColIdx::ZERO.as_usize(), 0);
}

#[test]
fn test_col_idx_new() {
    assert_eq!(ColIdx::new(80).as_usize(), 80);
}

#[test]
fn test_col_idx_width_of_ascii() {
    assert_eq!(ColIdx::width_of("hello").as_usize(), 5);
}

#[test]
fn test_col_idx_width_of_cjk() {
    assert_eq!(ColIdx::width_of("你好").as_usize(), 4);
}

#[test]
fn test_col_idx_width_of_emoji() {
    assert_eq!(ColIdx::width_of("🚀").as_usize(), 2);
}

#[test]
fn test_col_idx_add() {
    let c = ColIdx::new(10);
    assert_eq!(c.advance(5).as_usize(), 15);
}

#[test]
fn test_col_idx_sub() {
    assert_eq!(ColIdx::new(10) - ColIdx::new(3), 7);
}
