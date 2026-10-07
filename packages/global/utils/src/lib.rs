use sha2::{Digest, Sha256};

mod process;
pub use process::{configure_std_noninteractive, configure_tokio_noninteractive};

mod spawn_failure;
pub use spawn_failure::{describe_cwd_gone, describe_cwd_gone_failure};

#[cfg(test)]
#[path = "process_tests.rs"]
mod process_tests;

#[cfg(test)]
#[path = "spawn_failure_tests.rs"]
mod spawn_failure_tests;

/// 对多个已分隔字段生成稳定 SHA-256 十六进制摘要。
///
/// 域前缀后按 `be64(字段长度) + 字段字节` 分帧，因此字段边界参与摘要：
/// `["ab", "c"]` 与 `["a", "bc"]` 结果不同，domain 变化同样改变摘要。
/// 兼容性由 `lib_tests.rs` 中的黄金向量锁定。
pub fn stable_sha256_hex(domain: &[u8], fields: &[&[u8]]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(domain);
    for field in fields {
        hasher.update((field.len() as u64).to_be_bytes());
        hasher.update(field);
    }
    encode_hex(&hasher.finalize())
}

/// 计算一段连续原始字节流的标准 SHA-256 十六进制摘要（对整个输入直接哈希）。
///
/// 不做任何分帧或域隔离：多字段自行拼接会产生边界歧义
/// （`b"ab"+"c"` 与 `b"a"+"bc"` 摘要相同）。需要多字段、带域前缀的
/// 稳定标识时应改用 [`stable_sha256_hex`]。
pub fn sha256_hex(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    encode_hex(&hasher.finalize())
}

/// 将摘要字节编码为小写十六进制字符串。
fn encode_hex(digest: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut value = String::with_capacity(digest.len() * 2);
    for byte in digest {
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
    &s[..s.floor_char_boundary(max_bytes)]
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
    &s[start..] // allow unsafe_text_op: is_char_boundary aligned tail
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
