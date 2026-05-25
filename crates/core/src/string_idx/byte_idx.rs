//! 字节偏移索引。

/// 字节偏移。
///
/// 表示一个字符串中的字节位置，**必须**落在 UTF-8 char boundary 上。
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
pub struct ByteIdx(pub(crate) usize);

impl ByteIdx {
    pub const ZERO: Self = ByteIdx(0);

    /// 直接构造（不校验 char boundary，调用方需确保合法性）。
    pub fn new(n: usize) -> Self {
        ByteIdx(n)
    }

    /// 字符串末尾的字节位置，即 `s.len()`。
    pub fn end_of(s: &str) -> Self {
        ByteIdx(s.len())
    }

    /// 返回将字面量 `lit` 追加到当前字节位置之后的 ByteIdx。
    ///
    /// # 安全
    ///
    /// `lit` 必须是固定字面量（如 `🔬`），其在 `&str` 中的字节长度是确定的。
    pub fn after_str(self, lit: &str) -> Self {
        ByteIdx(self.0 + lit.len())
    }

    /// 在 `s` 中校验 `n` 是否是一个合法的 char boundary，若是则返回对应的 ByteIdx。
    pub fn new_at_boundary(s: &str, n: usize) -> Option<Self> {
        if s.is_char_boundary(n) {
            Some(ByteIdx(n))
        } else {
            None
        }
    }

    /// 取出裸 `usize`。
    pub fn as_usize(self) -> usize {
        self.0
    }

    /// 安全的字节偏移加法。
    pub fn checked_add(self, n: usize) -> Option<Self> {
        self.0.checked_add(n).map(ByteIdx)
    }
}

#[cfg(test)]
mod tests {
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
}
