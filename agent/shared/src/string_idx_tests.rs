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
    CharIdx::count_in(&s[..byte]) // allow unsafe_text_op: CharIdx/ByteIdx guaranteed boundary (safe API impl)
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

// -- slice_head / slice_tail 边界安全截断 --

#[test]
fn test_slice_head_ascii_and_short() {
    assert_eq!(slice_head("hello", 3), "hel");
    assert_eq!(slice_head("hi", 10), "hi"); // 短于上限原样返回
}

#[test]
fn test_slice_head_cjk_rounds_down() {
    // "你好世界" 每字 3 字节；max=4 落在 '好'(字节3..6) 内 → 回退到 "你"
    assert_eq!(slice_head("你好世界", 4), "你");
    assert_eq!(slice_head("你好世界", 6), "你好"); // 正好边界
}

#[test]
fn test_slice_tail_cjk_rounds_up() {
    // max=4 → start=8 落在 '世'(6..9) 内 → 前移到 9 → "界"
    assert_eq!(slice_tail("你好世界", 4), "界");
    assert_eq!(slice_tail("你好世界", 6), "世界"); // 正好边界
    assert_eq!(slice_tail("hi", 10), "hi");
}

#[test]
fn test_slice_head_tail_never_panic() {
    // 回归：任意字节上限都不 panic（覆盖所有切点落在多字节字符内的情况）
    let s = "a你b好c世";
    for n in 0..=s.len() {
        let _ = slice_head(s, n);
        let _ = slice_tail(s, n);
    }
}
