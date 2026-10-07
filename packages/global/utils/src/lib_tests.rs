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
    let source = "a你好🚀b";
    for max_bytes in 0..=source.len() + 2 {
        let _ = slice_head(source, max_bytes);
        let _ = slice_tail(source, max_bytes);
    }
}

#[test]
fn sha256_hex_of_hello_returns_standard_digest() {
    // SHA256("hello") = 2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824
    assert_eq!(
        sha256_hex(b"hello"),
        "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
    );
}

#[test]
fn sha256_hex_of_empty_slice_returns_standard_digest() {
    // SHA256("") = e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855
    assert_eq!(
        sha256_hex(&[]),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
}

// 独立工具（python hashlib 与 openssl dgst）计算并固定的黄金向量，
// 锁定 stable_sha256_hex 的编码路径：domain || be64(len(field)) || field。
const STABLE_GOLDEN_DOMAIN_V1: &[u8] = b"aemeath:stable_sha256_hex:v1";
const STABLE_GOLDEN_DOMAIN_V2: &[u8] = b"aemeath:stable_sha256_hex:v2";

#[test]
fn stable_sha256_hex_for_fixed_domain_and_parts_matches_golden_digest() {
    // sha256("aemeath:stable_sha256_hex:v1" || be64(11) || "project-key" || be64(5) || "alpha")
    assert_eq!(
        stable_sha256_hex(STABLE_GOLDEN_DOMAIN_V1, &[b"project-key", b"alpha"]),
        "9fb1eaead75a28d5aa01c83c4841acc853aba5a2b6f67a32f5f22a5c14548967"
    );
}

#[test]
fn stable_sha256_hex_frames_each_part_so_boundaries_change_digest() {
    let joined_first = stable_sha256_hex(STABLE_GOLDEN_DOMAIN_V1, &[b"ab", b"c"]);
    let joined_second = stable_sha256_hex(STABLE_GOLDEN_DOMAIN_V1, &[b"a", b"bc"]);
    assert_eq!(
        joined_first,
        "321968268c20be38d78306ace8b7d421146bce5f76680666f99ed85d546aaaf2"
    );
    assert_eq!(
        joined_second,
        "a66ec02c1731b3a91fc9f454e9dc4ed80e2a9943743c3a117e624b433e14c634"
    );
    assert_ne!(joined_first, joined_second);
}

#[test]
fn stable_sha256_hex_changes_digest_when_domain_changes() {
    assert_eq!(
        stable_sha256_hex(STABLE_GOLDEN_DOMAIN_V2, &[b"project-key", b"alpha"]),
        "77b679f06b0473e62600eb2911068915bd81080a6fb79974f870752a81d968e4"
    );
    assert_ne!(
        stable_sha256_hex(STABLE_GOLDEN_DOMAIN_V1, &[b"project-key", b"alpha"]),
        stable_sha256_hex(STABLE_GOLDEN_DOMAIN_V2, &[b"project-key", b"alpha"])
    );
}
