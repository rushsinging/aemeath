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
#[path = "byte_idx_tests.rs"]
mod tests;
