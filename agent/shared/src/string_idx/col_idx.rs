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
    pub fn advance(self, n: usize) -> Self {
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

impl ops::Add<usize> for ColIdx {
    type Output = Self;

    fn add(self, rhs: usize) -> Self::Output {
        ColIdx(self.0 + rhs)
    }
}

impl ops::Sub for ColIdx {
    type Output = usize;
    fn sub(self, rhs: ColIdx) -> usize {
        self.0.saturating_sub(rhs.0)
    }
}

#[cfg(test)]
#[path = "col_idx_tests.rs"]
mod tests;
