/// 从 checksums.txt 内容中查找指定文件名的 SHA256 值。
///
/// checksums.txt 格式（sha256sum 输出）：
/// ```text
/// a1b2c3...  aemeath-0.9.0-aarch64-apple-darwin.tar.gz
/// d4e5f6...  aemeath-0.9.0-x86_64-apple-darwin.tar.gz
/// ```
pub(super) fn parse_checksums(content: &str, archive_name: &str) -> Option<String> {
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        // 格式：`<hash>  <filename>`（两个空格分隔）
        let mut parts = line.splitn(2, |c: char| c.is_whitespace());
        let hash = parts.next()?.trim();
        let name = parts.next()?.trim();
        if name == archive_name {
            return Some(hash.to_lowercase());
        }
    }
    None
}

/// 计算字节数组的 SHA256 十六进制摘要。
pub(super) fn sha256_hex(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(data);
    let result = hasher.finalize();
    // 手动 hex 编码，避免额外依赖 hex crate
    let mut hex = String::with_capacity(64);
    for byte in result {
        hex.push_str(&format!("{byte:02x}"));
    }
    hex
}
