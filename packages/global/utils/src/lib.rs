pub(crate) const LOG_TARGET: &str = "aemeath:utils";
const _: &str = LOG_TARGET;
use sha2::{Digest, Sha256};

/// 对多个已分隔字段生成稳定 SHA-256 十六进制摘要。
pub fn stable_sha256_hex(domain: &[u8], fields: &[&[u8]]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(domain);
    for field in fields {
        hasher.update((field.len() as u64).to_be_bytes());
        hasher.update(field);
    }
    let digest = hasher.finalize();
    let mut value = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        write!(&mut value, "{byte:02x}").expect("writing to String cannot fail");
    }
    value
}

/// 从开头保留至多 `max_bytes` 字节，终点向前对齐到字符边界（不拆分 UTF-8）。
///
/// 用于头部预览截断。`max_bytes` 落在多字节字符内部时回退到该字符起始，
/// 杜绝 "byte index N is not a char boundary" panic。
pub fn slice_head(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// 从末尾保留至多 `max_bytes` 字节，起点向后对齐到字符边界（不拆分 UTF-8）。
///
/// 用于流式输出的 keep-tail 截断。`s.len() - max_bytes` 落在多字节字符内部时
/// 向后移到下一个字符起始，杜绝字符边界 panic。
pub fn slice_tail(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut start = s.len() - max_bytes;
    while start < s.len() && !s.is_char_boundary(start) {
        start += 1;
    }
    &s[start..]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_slice_head_ascii_and_short() {
        assert_eq!(slice_head("hello", 3), "hel");
        assert_eq!(slice_head("hi", 10), "hi");
    }

    #[test]
    fn test_slice_head_cjk_rounds_down() {
        assert_eq!(slice_head("你好世界", 4), "你");
        assert_eq!(slice_head("你好世界", 6), "你好");
    }

    #[test]
    fn test_slice_tail_preserves_ascii_tail() {
        assert_eq!(slice_tail("abcdef", 3), "def");
    }

    #[test]
    fn test_slice_tail_keeps_full_string_when_under_limit() {
        assert_eq!(slice_tail("hi", 10), "hi");
    }

    #[test]
    fn test_slice_tail_aligns_to_utf8_boundary() {
        assert_eq!(slice_tail("你好世界", 4), "界");
        assert_eq!(slice_tail("你好世界", 6), "世界");
    }

    #[test]
    fn test_slice_head_tail_never_panic() {
        let s = "a你好🚀b";
        for n in 0..=s.len() + 2 {
            let _ = slice_head(s, n);
            let _ = slice_tail(s, n);
        }
    }
}
