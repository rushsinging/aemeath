use super::*;

#[test]
fn empty_text_has_zero_tokens() {
    assert_eq!(estimate_tokens(""), 0);
}

#[test]
fn ascii_uses_latin_rate() {
    // 11 个非 CJK 字符 × 0.3 = 3.3 → 4
    assert_eq!(estimate_tokens("hello world"), 4);
}

#[test]
fn pure_cjk_uses_cjk_rate() {
    // 4 × 0.6 = 2.4 → 3
    assert_eq!(estimate_tokens("你好世界"), 3);
}

#[test]
fn same_char_count_can_have_different_token_cost() {
    assert_eq!(estimate_tokens("你好"), 2);
    assert_eq!(estimate_tokens("AB"), 1);
}

#[test]
fn mixed_cjk_and_latin_short_cases() {
    // 中(0.6) + A(0.3) = 0.9 → 1
    assert_eq!(estimate_tokens("中A"), 1);
    // 中(0.6) + AB(0.6) = 1.2 → 2
    assert_eq!(estimate_tokens("中AB"), 2);
    // 中文(1.2) + AB(0.6) = 1.8 → 2
    assert_eq!(estimate_tokens("中文AB"), 2);
    // 中文(1.2) + ABC(0.9) = 2.1 → 3
    assert_eq!(estimate_tokens("中文ABC"), 3);
    // 你好(1.2) + 全角逗号(0.6) + world!(1.8) = 3.6 → 4
    assert_eq!(estimate_tokens("你好，world!"), 4);
}

#[test]
fn mixed_text_aggregates_before_rounding() {
    // 单段「中文AB」= ceil(1.2 + 0.6) = 2；
    // 重复 100 次若逐段取整会得到 200，统一累计后应为 180。
    let text = "中文AB".repeat(100);
    assert_eq!(estimate_tokens(&text), 180);
}

#[test]
fn mixed_json_uses_same_character_rates() {
    // 2 个 CJK + 19 个非 CJK：1.2 + 5.7 = 6.9 → 7
    let json = r#"{"msg":"你好","code":0}"#;
    assert_eq!(estimate_json_tokens(json), 7);
}

#[test]
fn cjk_adjacent_scripts_are_counted_as_cjk() {
    // 平假名、片假名、韩文、全角标点、全角空格各 1 个：5 × 0.6 = 3
    assert_eq!(estimate_tokens("あア한，　"), 3);
}

#[test]
fn custom_rates_are_clamped_to_safe_range() {
    assert_eq!(estimate_tokens_with_char_rates("abcdefghij", 0.01, 0.01), 1);
    assert_eq!(estimate_tokens_with_char_rates("你好", 5.0, 5.0), 4);
}

#[test]
fn legacy_ratio_maps_to_latin_rate() {
    // bytes_per_token=4 → latin rate 0.25；"abcd" → 1
    assert_eq!(estimate_tokens_with_ratio("abcd", 4.0), 1);
}

#[test]
fn truncate_keeps_text_within_budget_unchanged() {
    assert_eq!(
        truncate_to_token_budget("正常内容", 20, "…（截断）"),
        "正常内容"
    );
}

#[test]
fn truncate_mixed_text_never_exceeds_budget() {
    let marker = "…（截断）";
    let text = "中文AB".repeat(100);
    let truncated = truncate_to_token_budget(&text, 20, marker);
    assert!(truncated.ends_with(marker));
    assert!(estimate_tokens(&truncated) <= 20);
    assert_ne!(truncated, text);
}

#[test]
fn truncate_english_allows_more_chars_than_cjk_for_same_token_budget() {
    let marker = "…（截断）";
    let cjk = truncate_to_token_budget(&"规".repeat(200), 40, marker);
    let latin = truncate_to_token_budget(&"x".repeat(200), 40, marker);
    assert!(estimate_tokens(&cjk) <= 40);
    assert!(estimate_tokens(&latin) <= 40);
    assert!(
        latin.chars().count() > cjk.chars().count(),
        "同等 token 预算下英文应容纳更多字符"
    );
}
