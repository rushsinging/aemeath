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
    // #1773：摘要预留由 2% 放宽到 5%，让压缩后的摘要能承载更多历史。
    assert_eq!(summary_budget(200_000), 10_000);
    assert_eq!(effective_context_window(200_000, 16_000), 174_000);
}

#[test]
fn threshold_uses_only_effective_window_safety_ratio() {
    assert_eq!(autocompact_threshold(200_000, 16_000, 0.8), 139_200);
}

/// 配置化的触发阈值比例：effective(200_000, 16_000) = 174_000，
/// ratio 0.9 → 156_600（默认 0.8 → 139_200）。
#[test]
fn threshold_honors_configured_ratio() {
    assert_eq!(autocompact_threshold(200_000, 16_000, 0.9), 156_600);
    assert_eq!(autocompact_threshold(200_000, 16_000, 0.5), 87_000);
}

/// 越界 ratio clamp 到安全区间 [0.5, 0.95]：
/// 下限防 compact 风暴（#1626），上限防估算缓冲归零。
#[test]
fn threshold_ratio_clamped_into_safe_range() {
    assert_eq!(
        autocompact_threshold(200_000, 16_000, 0.1),
        autocompact_threshold(200_000, 16_000, 0.5)
    );
    assert_eq!(
        autocompact_threshold(200_000, 16_000, 0.99),
        autocompact_threshold(200_000, 16_000, 0.95)
    );
}

#[test]
fn effective_window_saturates_when_reservations_exceed_context_size() {
    // #1626：max_output 参与预留前 clamp 到窗口 25% 上限，
    // 预留超出窗口的场景不再让 effective 归零（恒触发 compact 风暴根因）。
    // 1_000 窗口：clamp(2_000 -> 250)，effective = 1_000 - 50 - 250 = 700。
    assert_eq!(effective_context_window(1_000, 2_000), 700);
    assert!(autocompact_threshold(1_000, 2_000, 0.8) > 0);
}

/// #1626 复现：8k 窗口 + 默认 max_output 8192 时 effective=0、threshold=0，
/// 任意一轮对话即恒触发 auto-compact。护栏生效后 threshold 永不为 0。
#[test]
fn threshold_never_zero_for_short_window_with_default_output() {
    assert!(autocompact_threshold(8_192, 8_192, 0.8) > 0);
    assert!(autocompact_threshold(16_384, 8_192, 0.8) > 0);
    assert!(autocompact_threshold(32_768, 8_192, 0.8) > 0);
}

/// #1626：max_output 预留 clamp 到窗口 25% 上限。
/// 16_384 窗口：clamp(8_192 -> 4_096)，effective = 16_384 - 819 - 4_096 = 11_469
/// （#1773 后摘要预留由 327（2%）放宽到 819（5%））。
#[test]
fn max_output_clamped_to_quarter_of_window() {
    assert_eq!(effective_context_window(16_384, 8_192), 11_469);
    assert_eq!(autocompact_threshold(16_384, 8_192, 0.8), 9_175);
}

/// #1626：clamp 不影响大窗口常规配置（8k output 远小于 200k 的 25%）。
#[test]
fn clamp_keeps_large_window_behavior_unchanged() {
    assert_eq!(clamped_max_output(200_000, 16_000), 16_000);
    assert_eq!(
        effective_context_window(200_000, 16_000),
        200_000 - summary_budget(200_000) - 16_000,
    );
}

#[test]
fn compact_tail_token_cap_is_three_percent_of_window() {
    // #1688 起按窗口比例封顶；#1773 由 5% 收紧为 3%——尾部只保留最近
    // 上下文，摘要预算随之放大到 5% 承载更多历史。
    assert_eq!(compact_tail_token_cap(200_000), 6_000);
    assert_eq!(compact_tail_token_cap(300_000), 9_000);
    assert_eq!(compact_tail_token_cap(1_000_000), 30_000);
}

/// 3% 无法用整数除法简洁表达：`context_size / 33` 这类魔法除数会在
/// 33 与 34 之间反复横跳（33.3% 与 2.94% 混用），因此按百分数直乘。
#[test]
fn compact_tail_token_cap_keeps_three_percent_at_both_sides_of_the_magic_divisor() {
    // 33×3=99 < 100，34×3=102 ≥ 100：魔法除数在这两个窗口上给出不同比例。
    assert_eq!(compact_tail_token_cap(33), 0);
    assert_eq!(33 / 33, 1, "the magic divisor would round 33 up to 1");
    assert_eq!(compact_tail_token_cap(34), 1);
    // 大窗口上精确收敛到 3%，不因整数截断偏低。
    assert_eq!(compact_tail_token_cap(10_000), 300);
    assert_eq!(compact_tail_token_cap(3_300), 99);
}

/// 短窗口下 3% 可能取整为 0：此时 tail 候选仍受条数上限约束，
/// 不允许 clamp 成 1 制造虚假预算。
#[test]
fn compact_tail_token_cap_may_round_to_zero_on_tiny_windows() {
    assert_eq!(compact_tail_token_cap(33), 0);
    assert_eq!(compact_tail_token_cap(0), 0);
}
