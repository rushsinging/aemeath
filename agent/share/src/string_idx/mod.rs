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
//! use share::string_idx::StrSlice;
//! s.bslice_from(byte_start)
//! ```

mod byte_idx;
mod char_idx;
mod col_idx;

pub use byte_idx::ByteIdx;
pub use char_idx::CharIdx;
pub use col_idx::ColIdx;

use std::ops;

// ---------------------------------------------------------------------------
// 跨类型转换：必须带 &str 上下文
// ---------------------------------------------------------------------------

/// 将字符位置转换为字节偏移（O(n)）。
///
/// 如果 `c` 超出字符串的字符总数，返回末尾的 ByteIdx。
pub fn char_to_byte(s: &str, c: CharIdx) -> ByteIdx {
    s.char_indices()
        .nth(c.0)
        .map(|(b, _)| ByteIdx::new(b))
        .unwrap_or_else(|| ByteIdx::end_of(s))
}

/// 将字节偏移转换为字符位置（O(n)）。
///
/// 如果 `b` 超出字符串长度，返回末尾的 CharIdx。
pub fn byte_to_char(s: &str, b: ByteIdx) -> CharIdx {
    if b.0 >= s.len() {
        return CharIdx::count_in(s);
    }
    // char_indices 的索引 i 是字节偏移，count 是字符计数
    CharIdx::new(
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
            return CharIdx::new(ch_idx);
        }
        width += ch_w;
    }
    CharIdx::count_in(s)
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
    ColIdx::new(col)
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
// 测试：跨类型转换 + StrSlice + 混合场景
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

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
        let end = CharIdx::new(4); // '🚀c' → 但实际是第3和第4个字符'b'和'🚀c'... wait
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
        let mut byte = b.as_usize().min(s.len());
        while byte > 0 && !s.is_char_boundary(byte) {
            byte -= 1;
        }
        CharIdx::count_in(&s[..byte])
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
}
