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
        let ch_w = ch.width().unwrap_or(1);
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
        col += ch.width().unwrap_or(1);
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
        &self[range.start.0..range.end.0] // allow unsafe_text_op: CharIdx/ByteIdx guaranteed boundary (safe API impl)
    }

    fn bslice_to(&self, end: ByteIdx) -> &str {
        &self[..end.0] // allow unsafe_text_op: CharIdx/ByteIdx guaranteed boundary (safe API impl)
    }

    fn bslice_from(&self, start: ByteIdx) -> &str {
        &self[start.0..] // allow unsafe_text_op: CharIdx/ByteIdx guaranteed boundary (safe API impl)
    }

    fn cslice(&self, range: ops::Range<CharIdx>) -> &str {
        let byte_start = char_to_byte(self, range.start);
        let byte_end = char_to_byte(self, range.end);
        &self[byte_start.0..byte_end.0] // allow unsafe_text_op: CharIdx/ByteIdx guaranteed boundary (safe API impl)
    }
}

pub use utils::{slice_head, slice_tail};

// ---------------------------------------------------------------------------
// 测试：跨类型转换 + StrSlice + 混合场景
// ---------------------------------------------------------------------------

#[cfg(test)]
#[path = "string_idx_tests.rs"]
mod tests;
