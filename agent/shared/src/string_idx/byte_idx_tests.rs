use super::*;

#[test]
fn test_byte_idx_zero() {
    assert_eq!(ByteIdx::ZERO.as_usize(), 0);
}

#[test]
fn test_byte_idx_new() {
    assert_eq!(ByteIdx::new(42).as_usize(), 42);
}

#[test]
fn test_byte_idx_end_of_ascii() {
    assert_eq!(ByteIdx::end_of("hello").as_usize(), 5);
}

#[test]
fn test_byte_idx_end_of_cjk() {
    assert_eq!(ByteIdx::end_of("你好").as_usize(), 6);
}

#[test]
fn test_byte_idx_after_str() {
    let start = ByteIdx::new(10);
    let next = start.after_str("🔬");
    assert_eq!(next.as_usize(), 14); // 10 + "🔬".len()
}

#[test]
fn test_byte_idx_new_at_boundary_valid() {
    let s = "你好世界";
    let b = ByteIdx::new_at_boundary(s, 3).unwrap();
    assert_eq!(b.as_usize(), 3);
}

#[test]
fn test_byte_idx_new_at_boundary_invalid() {
    let s = "你好世界";
    assert!(ByteIdx::new_at_boundary(s, 1).is_none());
    assert!(ByteIdx::new_at_boundary(s, 2).is_none());
}

#[test]
fn test_byte_idx_checked_add_overflow() {
    let b = ByteIdx::new(usize::MAX);
    assert!(b.checked_add(1).is_none());
}

#[test]
fn test_byte_idx_checked_add_ok() {
    let b = ByteIdx::new(10);
    assert_eq!(b.checked_add(5).unwrap().as_usize(), 15);
}

#[test]
fn test_after_str_on_byte_idx() {
    let start = ByteIdx::end_of("prefix_");
    let after = start.after_str("suffix");
    assert_eq!(after.as_usize(), 7 + 6); // prefix_(7) + suffix(6)
}
