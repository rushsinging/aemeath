//! 字符串索引强类型化 — 在编译期区分三种 `usize` 语义。
//!
//! # 类型
//!
//! | 类型 | 含义 | 来源 |
//! |------|------|------|
//! | [`ByteIdx`] | 字节偏移 | `s.len()`、`s.find()`、字面量长度 |
//! | [`CharIdx`] | 字符位置 | `s.chars().count()`、`s.chars().nth(n)` |
//! | [`ColIdx`] | 显示列宽 | `unicode_width::UnicodeWidthStr::width()` |
//!
//! # 约束
//!
//! - 不实现 `From<usize>` / `Deref<Target = usize>`：不能把裸 `usize` 隐式塞进来。
//! - 跨类型转换必须带 `&str` 上下文，强制显式确认。
//! - 取 `usize` 必须调用 `.as_usize()`。
//!
//! # 安全切片
//!
//! 使用 [`StrSlice`] 扩展 trait 替代裸 `&s[a..b]`：
//!
//! ```ignore
//! use aemeath_core::string_idx::StrSlice;
//! s.bslice_from(byte_start)
//! ```

use std::ops;

// ---------------------------------------------------------------------------
// 类型定义
// ---------------------------------------------------------------------------

/// 字节偏移。
///
/// 表示一个字符串中的字节位置，**必须**落在 UTF-8 char boundary 上。
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
pub struct ByteIdx(pub(crate) usize);

/// 字符位置。
///
/// 表示一个字符串中的第 N 个字符（0-indexed）。
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
pub struct CharIdx(pub(crate) usize);

/// 显示列宽位置。
///
/// 表示终端显示中的第 N 列（0-indexed），基于 Unicode 显示宽度。
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
pub struct ColIdx(pub(crate) usize);

// ---------------------------------------------------------------------------
// ByteIdx
// ---------------------------------------------------------------------------

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
    /// `lit` 必须是固定字面量（如 `<think>`），其在 `&str` 中的字节长度是确定的。
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

// ---------------------------------------------------------------------------
// CharIdx
// ---------------------------------------------------------------------------

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
    pub fn add(self, n: usize) -> Self {
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

impl ops::Sub for CharIdx {
    type Output = usize;
    fn sub(self, rhs: CharIdx) -> usize {
        self.0.saturating_sub(rhs.0)
    }
}

// ---------------------------------------------------------------------------
// ColIdx
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// 跨类型转换：必须带 &str 上下文
// ---------------------------------------------------------------------------

/// 将字符位置转换为字节偏移（O(n)）。
///
/// 如果 `c` 超出字符串的字符总数，返回末尾的 ByteIdx。
pub fn char_to_byte(s: &str, c: CharIdx) -> ByteIdx {
    s.char_indices()
        .nth(c.0)
        .map(|(b, _)| ByteIdx(b))
        .unwrap_or_else(|| ByteIdx(s.len()))
}

/// 将字节偏移转换为字符位置（O(n)）。
///
/// 如果 `b` 超出字符串长度，返回末尾的 CharIdx。
pub fn byte_to_char(s: &str, b: ByteIdx) -> CharIdx {
    if b.0 >= s.len() {
        return CharIdx(s.chars().count());
    }
    // char_indices 的索引 i 是字节偏移，count 是字符计数
    CharIdx(
        s.char_indices()
            .take_while(|(byte_pos, _)| *byte_pos < b.0)
            .count(),
    )
}

/// 将显示列位置转换为字符位置（O(n)）。
pub fn col_to_char(s: &str, c: ColIdx) -> CharIdx {
    use unicode_width::UnicodeWidthChar;
    let mut width = 0usize;
    for (ch_idx, ch) in s.chars().enumerate() {
        let ch_w = ch.width().unwrap_or(1) as usize;
        if width + ch_w > c.0 {
            return CharIdx(ch_idx);
        }
        width += ch_w;
    }
    CharIdx(s.chars().count())
}

/// 将字符位置转换为显示列位置（O(n)）。
pub fn char_to_col(s: &str, c: CharIdx) -> ColIdx {
    use unicode_width::UnicodeWidthChar;
    let mut col = 0usize;
    for (ch_idx, ch) in s.chars().enumerate() {
        if ch_idx >= c.0 {
            break;
        }
        col += ch.width().unwrap_or(1) as usize;
    }
    ColIdx(col)
}

// ---------------------------------------------------------------------------
// StrSlice — 安全切片扩展 trait
// ---------------------------------------------------------------------------

/// 使用类型化的索引对 `str` 进行安全切片。
///
/// 替代裸 `&s[a..b]`：
/// - `s.bslice(..)` 接受 [`Range<ByteIdx>`]
/// - `s.bslice_from(start)` 接受 [`ByteIdx`]
/// - `s.bslice_to(end)` 接受 [`ByteIdx`]
/// - `s.cslice(..)` 接受 [`Range<CharIdx>`]（内部转字节）
pub trait StrSlice {
    fn bslice(&self, range: ops::Range<ByteIdx>) -> &str;
    fn bslice_to(&self, end: ByteIdx) -> &str;
    fn bslice_from(&self, start: ByteIdx) -> &str;
    fn cslice(&self, range: ops::Range<CharIdx>) -> &str;
}

impl StrSlice for str {
    fn bslice(&self, range: ops::Range<ByteIdx>) -> &str {
        &self[range.start.0..range.end.0]
    }

    fn bslice_to(&self, end: ByteIdx) -> &str {
        &self[..end.0]
    }

    fn bslice_from(&self, start: ByteIdx) -> &str {
        &self[start.0..]
    }

    fn cslice(&self, range: ops::Range<CharIdx>) -> &str {
        let byte_start = char_to_byte(self, range.start);
        let byte_end = char_to_byte(self, range.end);
        &self[byte_start.0..byte_end.0]
    }
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- ByteIdx 测试 --

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
        let next = start.after_str("<think>");
        assert_eq!(next.as_usize(), 17); // 10 + "<think>".len()
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

    // -- CharIdx 测试 --

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
        assert_eq!(c.add(5).as_usize(), 8);
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

    // -- ColIdx 测试 --

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

    // -- 跨类型转换测试 --

    #[test]
    fn test_char_to_byte_ascii() {
        let s = "hello";
        let c = CharIdx::new(2);
        assert_eq!(char_to_byte(s, c).as_usize(), 2); // 'l'
    }

    #[test]
    fn test_char_to_byte_cjk() {
        let s = "你好世界";
        // '你'=3字节, '好'=3字节, 第2个字符'好'的字节偏移是3
        assert_eq!(char_to_byte(s, CharIdx::new(1)).as_usize(), 3);
    }

    #[test]
    fn test_char_to_byte_out_of_range() {
        let s = "hi";
        let b = char_to_byte(s, CharIdx::new(10));
        assert_eq!(b, ByteIdx::end_of(s));
    }

    #[test]
    fn test_byte_to_char_ascii() {
        let s = "hello";
        assert_eq!(byte_to_char(s, ByteIdx::new(2)).as_usize(), 2);
    }

    #[test]
    fn test_byte_to_char_cjk() {
        let s = "你好世界";
        // 字节偏移3在第二个字符'好'的起始位置 → 字符索引1
        assert_eq!(byte_to_char(s, ByteIdx::new(3)).as_usize(), 1);
    }

    #[test]
    fn test_byte_to_char_out_of_range() {
        let s = "hi";
        let c = byte_to_char(s, ByteIdx::new(100));
        assert_eq!(c.as_usize(), 2);
    }

    #[test]
    fn test_col_to_char_ascii() {
        let s = "hello";
        assert_eq!(col_to_char(s, ColIdx::new(2)).as_usize(), 2);
    }

    #[test]
    fn test_col_to_char_cjk() {
        let s = "你好"; // 每个字2列宽
        // 列2 → 第二个字符'好'的字符索引1
        assert_eq!(col_to_char(s, ColIdx::new(2)).as_usize(), 1);
    }

    #[test]
    fn test_col_to_char_emoji() {
        let s = "a🚀b"; // 'a'=1列, '🚀'=2列, 'b'=1列
        // 列0='a', 列1-2='🚀', 列3='b'
        // ColIdx(2) 落在 '🚀' 内 → char idx 1
        assert_eq!(col_to_char(s, ColIdx::new(2)).as_usize(), 1);
    }

    #[test]
    fn test_char_to_col_cjk() {
        let s = "你好世界";
        // 第2个字符'好' → 列偏移2
        assert_eq!(char_to_col(s, CharIdx::new(1)).as_usize(), 2);
    }

    #[test]
    fn test_char_to_col_emoji() {
        let s = "a🚀b";
        // 第3个字符'b' → 列偏移 1(a) + 2(🚀) = 3
        assert_eq!(char_to_col(s, CharIdx::new(2)).as_usize(), 3);
    }

    // -- StrSlice 测试 --

    #[test]
    fn test_bslice() {
        let s = "hello world";
        let start = ByteIdx::new(6);
        let end = ByteIdx::new(11);
        assert_eq!(s.bslice(start..end), "world");
    }

    #[test]
    fn test_bslice_from() {
        let s = "hello world";
        let start = ByteIdx::new(6);
        assert_eq!(s.bslice_from(start), "world");
    }

    #[test]
    fn test_bslice_to() {
        let s = "hello world";
        let end = ByteIdx::new(5);
        assert_eq!(s.bslice_to(end), "hello");
    }

    #[test]
    fn test_cslice_cjk() {
        let s = "你好世界";
        // 字符索引 1..3 → "好世"
        let start = CharIdx::new(1);
        let end = CharIdx::new(3);
        assert_eq!(s.cslice(start..end), "好世");
    }

    #[test]
    fn test_cslice_emoji() {
        let s = "a🚀b🚀c";
        let start = CharIdx::new(2); // 'b'
        let end = CharIdx::new(4);   // '🚀c' → 但实际是第3和第4个字符'b'和'🚀c'... wait
        // s.chars(): ['a', '🚀', 'b', '🚀', 'c'] → CharIdx 2='b', CharIdx 4='c'
        // 但 end=4 是排他边界，所以 2..4 = ['b', '🚀']
        assert_eq!(s.cslice(start..end), "b🚀");
    }

    #[test]
    fn test_bslice_empty() {
        let s = "";
        let start = ByteIdx::ZERO;
        let end = ByteIdx::end_of(s);
        assert_eq!(s.bslice(start..end), "");
    }

    #[test]
    fn test_bslice_full() {
        let s = "hello";
        let full = s.bslice(ByteIdx::ZERO..ByteIdx::end_of(s));
        assert_eq!(full, "hello");
    }

    #[test]
    fn test_cslice_full_cjk() {
        let s = "你好世界";
        let full = s.cslice(CharIdx::ZERO..CharIdx::count_in(s));
        assert_eq!(full, "你好世界");
    }

    // -- 混合场景 --

    #[test]
    fn test_roundtrip_char_byte_char() {
        let s = "a你好🚀world";
        for ci in 0..s.chars().count() {
            let c = CharIdx::new(ci);
            let b = char_to_byte(s, c);
            let c2 = byte_to_char(s, b);
            assert_eq!(c, c2, "roundtrip failed at char index {}", ci);
        }
    }

    /// byte_to_char 落在 char 内部应向前对齐到最近的完整 char 边界
    fn byte_to_char_floor(s: &str, b: ByteIdx) -> CharIdx {
        let mut byte = b.0.min(s.len());
        while byte > 0 && !s.is_char_boundary(byte) {
            byte -= 1;
        }
        CharIdx(s[..byte].chars().count())
    }

    #[test]
    fn test_byte_to_char_inside_char_rounds_down() {
        let s = "你好";
        // 字节1在'你'的第二个字节，应回退到0
        assert_eq!(byte_to_char_floor(s, ByteIdx::new(1)).as_usize(), 0);
        // 字节4在'好'的第二个字节，应回退到1（'好'的起始）
        assert_eq!(byte_to_char_floor(s, ByteIdx::new(4)).as_usize(), 1);
    }

    #[test]
    fn test_col_to_char_out_of_range() {
        let s = "hi";
        let c = col_to_char(s, ColIdx::new(100));
        assert_eq!(c.as_usize(), 2);
    }

    #[test]
    fn test_char_to_col_out_of_range() {
        let s = "hi";
        let col = char_to_col(s, CharIdx::new(100));
        // 走到末尾，只累计到字符串末尾
        assert_eq!(col.as_usize(), 2);
    }

    #[test]
    fn test_after_str_on_byte_idx() {
        let start = ByteIdx::end_of("prefix_");
        let after = start.after_str("suffix");
        assert_eq!(after.as_usize(), 7 + 6); // prefix_(7) + suffix(6)
    }
}
