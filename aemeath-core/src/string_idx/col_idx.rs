//! 显示列宽位置索引。

use std::ops;

/// 显示列宽位置。
///
/// 表示终端显示中的第 N 列（0-indexed），基于 Unicode 显示宽度。
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
pub struct ColIdx(pub(crate) usize);

impl ColIdx {
    pub const ZERO: Self = ColIdx(0);

    /// 直接构造。
    pub fn new(n: usize) -> Self {
        ColIdx(n)
    }

    /// 计算 `s` 的 Unicode 显示宽度。
    pub fn width_of(s: &str) -> Self {
        ColIdx(unicode_width::UnicodeWidthStr::width(s))
    }

    /// 前进 `n` 列。
    pub fn add(self, n: usize) -> Self {
        ColIdx(self.0 + n)
    }

    /// 两个 ColIdx 之间的距离。
    pub fn saturating_sub(self, other: ColIdx) -> usize {
        self.0.saturating_sub(other.0)
    }

    /// 取出裸 `usize`。
    pub fn as_usize(self) -> usize {
        self.0
    }
}

impl ops::Sub for ColIdx {
    type Output = usize;
    fn sub(self, rhs: ColIdx) -> usize {
        self.0.saturating_sub(rhs.0)
    }
}

#[cfg(test)]
mod tests {
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
        assert_eq!(c.add(5).as_usize(), 15);
    }

    #[test]
    fn test_col_idx_sub() {
        assert_eq!(ColIdx::new(10) - ColIdx::new(3), 7);
    }
}
