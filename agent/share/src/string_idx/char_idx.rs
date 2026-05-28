//! 字符位置索引。

use std::ops;

/// 字符位置。
///
/// 表示一个字符串中的第 N 个字符（0-indexed）。
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
pub struct CharIdx(pub(crate) usize);

impl CharIdx {
    pub const ZERO: Self = CharIdx(0);

    /// 直接构造。
    pub fn new(n: usize) -> Self {
        CharIdx(n)
    }

    /// 统计 `s` 中的字符数。
    pub fn count_in(s: &str) -> Self {
        CharIdx(s.chars().count())
    }

    /// 前进 `n` 个字符（不校验边界）。
    pub fn advance(self, n: usize) -> Self {
        CharIdx(self.0 + n)
    }

    /// 安全前进，不超过 `s` 的字符总数。
    pub fn checked_add(self, n: usize, s: &str) -> Option<Self> {
        let total = s.chars().count();
        let result = self.0 + n;
        if result <= total {
            Some(CharIdx(result))
        } else {
            None
        }
    }

    /// 两个 CharIdx 之间的距离（字符数）。
    pub fn saturating_sub(self, other: CharIdx) -> usize {
        self.0.saturating_sub(other.0)
    }

    /// 取出裸 `usize`。
    pub fn as_usize(self) -> usize {
        self.0
    }
}

impl ops::Add<usize> for CharIdx {
    type Output = Self;

    fn add(self, rhs: usize) -> Self::Output {
        CharIdx(self.0 + rhs)
    }
}

impl ops::Sub for CharIdx {
    type Output = usize;
    fn sub(self, rhs: CharIdx) -> usize {
        self.0.saturating_sub(rhs.0)
    }
}

#[cfg(test)]
mod tests {
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
}
