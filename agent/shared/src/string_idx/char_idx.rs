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
#[path = "char_idx_tests.rs"]
mod tests;
