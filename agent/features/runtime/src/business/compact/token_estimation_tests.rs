use super::*;

#[test]
fn test_estimate_ascii() {
    // ASCII: ~4 chars per token
    let tokens = estimate_tokens("hello world");
    assert!(tokens > 0);
    // "hello world" is 11 chars, should be about 3 tokens
    assert!((3..=5).contains(&tokens));
}

#[test]
fn test_estimate_cjk() {
    // CJK: ~2 tokens per char
    let tokens = estimate_tokens("你好世界");
    assert!(tokens > 0);
    // 4 CJK chars should be about 8 tokens (with safety margin)
    assert!(tokens >= 8);
}

#[test]
fn test_context_usage() {
    let est = TokenEstimation::new(1000);
    let usage = est.usage_stats(&[], "Hello world");
    assert!(usage.total_tokens > 0);
    assert!(!usage.needs_compaction);
}

#[test]
fn test_format_tokens() {
    assert_eq!(format_tokens(500), "500");
    assert_eq!(format_tokens(1500), "1.5k");
    assert_eq!(format_tokens(15000), "15k");
    assert_eq!(format_tokens(1500000), "1.5m");
}
