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
    // CJK: ~1 token per char（主流 tokenizer 实测 0.7-1.0 tokens/char；
    // #1500 复现：按 2 tokens/char + 1.33x margin 估算高估 ~2.7 倍，
    // 导致 heuristic 判定在 ctx 43% 时误触发 compact）
    let tokens = estimate_tokens("你好世界");
    assert_eq!(tokens, 4);
}

#[test]
fn test_estimate_ascii_no_safety_margin_inflation() {
    // 长 ASCII 文本 ≈ len/4 tokens，不再叠加 1.33x safety margin——
    // threshold 已含 0.8 安全系数，双重保险造成估算系统性高估。
    let text = "the quick brown fox jumps over the lazy dog. ".repeat(50);
    let tokens = estimate_tokens(&text);
    assert_eq!(tokens, text.len().div_ceil(4));
}

#[test]
fn test_estimate_json_no_inflated_ratio() {
    // JSON（tool input / tool schemas）按 ~4 bytes/token 估算；
    // #1500 复现：旧实现按 2 bytes/token × 4/3 高估 ~2.7 倍。
    let json = r#"{"name":"Read","path":"src/main.rs","line":10}"#;
    let tokens = estimate_json_tokens(json);
    assert_eq!(tokens, json.len().div_ceil(4));
}

#[test]
fn effective_window_reserves_guidance_summary_and_output_independently() {
    assert_eq!(summary_budget(200_000), 4_000);
    assert_eq!(effective_context_window(200_000, 16_000), 180_000);
}

#[test]
fn threshold_uses_only_effective_window_safety_ratio() {
    assert_eq!(autocompact_threshold(200_000, 16_000), 144_000);
}

#[test]
fn effective_window_saturates_when_reservations_exceed_context_size() {
    assert_eq!(effective_context_window(1_000, 2_000), 0);
    assert_eq!(autocompact_threshold(1_000, 2_000), 0);
}
