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

#[test]
fn test_needs_compaction_actual_no_cache_no_reasoning() {
    // 没有 cached_tokens 和 reasoning_tokens
    assert!(!needs_compaction_actual(
        50000, 10000, None, None, 1_048_576
    ));
    assert!(needs_compaction_actual(
        900000, 200000, None, None, 1_048_576
    ));
}

#[test]
fn normalized_total_above_threshold_needs_compaction() {
    assert!(needs_compaction_total(900_000, 1_048_576));
}

#[test]
fn normalized_total_at_or_below_threshold_does_not_need_compaction() {
    let threshold = autocompact_threshold(1_048_576, 8192) as u64;
    assert!(!needs_compaction_total(threshold, 1_048_576));
}

#[test]
fn test_needs_compaction_actual_with_cached_tokens() {
    // 有 cached_tokens，但不扣除（cached tokens 仍占用 context window）
    // input=100000, cached=80000, total=110000 (100000 + 10000)
    assert!(!needs_compaction_actual(
        100000,
        10000,
        Some(80000),
        None,
        1_048_576
    ));

    // input=100000, cached=0, total=110000 (100000 + 10000)
    assert!(!needs_compaction_actual(
        100000,
        10000,
        Some(0),
        None,
        1_048_576
    ));
}

#[test]
fn test_needs_compaction_actual_reasoning_not_double_counted() {
    // reasoning_tokens 是 output_tokens 的子集（completion_tokens_details
    // .reasoning_tokens ⊂ completion_tokens），不应重复累加。
    // 即使 reasoning 很大，只要 input + output 没超 threshold，就不应触发。
    // input=50000, output=10000, reasoning=970000 → total=60000（不含 reasoning）
    // threshold = 821,907, 60000 < threshold -> false
    assert!(!needs_compaction_actual(
        50000,
        10000,
        None,
        Some(970000),
        1_048_576
    ));

    // 对照：无 reasoning 时同样 input+output 也不触发
    assert!(!needs_compaction_actual(
        50000, 10000, None, None, 1_048_576
    ));

    // reasoning 不影响判定：只有 input+output 超 threshold 才触发
    // input=1000000, output=50000, total=1050000 > 821,907 -> true
    assert!(needs_compaction_actual(
        1000000,
        50000,
        None,
        Some(50000),
        1_048_576
    ));
}

#[test]
fn test_needs_compaction_actual_with_both() {
    // cached 和 reasoning 同时存在：cached 不扣除，reasoning 不重复累加
    // input=200000, cached=150000 (不扣除), output=10000, reasoning=830000
    // total = 200000 + 10000 = 210000（不含 reasoning）
    // threshold = 821,907, 210000 < threshold -> false
    assert!(!needs_compaction_actual(
        200000,
        10000,
        Some(150000),
        Some(830000),
        1_048_576
    ));

    // input+output 超 threshold 时触发（reasoning 仍是 output 子集）
    // input=1000000, output=50000, total=1050000 > 821,907 -> true
    assert!(needs_compaction_actual(
        1000000,
        50000,
        Some(150000),
        Some(830000),
        1_048_576
    ));
}

#[test]
fn test_needs_compaction_actual_cached_greater_than_input() {
    // cached_tokens 大于 input_tokens（不扣除）
    // input=10000, cached=50000 (不扣除), output=10000
    // total = 10000 + 10000 = 20000
    assert!(!needs_compaction_actual(
        10000,
        10000,
        Some(50000),
        None,
        1_048_576
    ));
}

#[test]
fn test_compaction_urgency_no_cache_no_reasoning() {
    // 没有 cached_tokens 和 reasoning_tokens
    // effective_context_window(1,048,576, 8192) = 1,040,384
    // 100000 / 1,040,384 = 9.6% -> level 0
    assert_eq!(compaction_urgency(100000, None, None, 1_048_576), 0);
    // 700000 / 1,040,384 = 67.3% -> level 0
    assert_eq!(compaction_urgency(700000, None, None, 1_048_576), 0);
    // 730000 / 1,040,384 = 70.2% -> level 1
    assert_eq!(compaction_urgency(730000, None, None, 1_048_576), 1);
    // 800000 / 1,040,384 = 76.9% -> level 1
    assert_eq!(compaction_urgency(800000, None, None, 1_048_576), 1);
    // 840000 / 1,040,384 = 80.7% -> level 2
    assert_eq!(compaction_urgency(840000, None, None, 1_048_576), 2);
    // 940000 / 1,040,384 = 90.4% -> level 3
    assert_eq!(compaction_urgency(940000, None, None, 1_048_576), 3);
}

#[test]
fn test_compaction_urgency_with_cached_tokens() {
    // 有 cached_tokens，但不扣除（cached tokens 仍占用 context window）
    // input=100000, cached=80000 (不扣除), 100000/1,040,384 = 9.6% -> level 0
    assert_eq!(compaction_urgency(100000, Some(80000), None, 1_048_576), 0);

    // input=900000, cached=200000 (不扣除), 900000/1,040,384 = 86.5% -> level 2
    assert_eq!(compaction_urgency(900000, Some(200000), None, 1_048_576), 2);

    // input=900000, cached=100000 (不扣除), 900000/1,040,384 = 86.5% -> level 2
    assert_eq!(compaction_urgency(900000, Some(100000), None, 1_048_576), 2);
}

#[test]
fn test_compaction_urgency_reasoning_not_double_counted() {
    // reasoning_tokens 是 output 的子集，不应累加到当前占用
    // input=100000, reasoning=740000 → 当前占用 = input = 100000
    // 100000/1,040,384 = 9.6% -> level 0（而非旧逻辑的 80.7% -> level 2）
    assert_eq!(compaction_urgency(100000, None, Some(740000), 1_048_576), 0);

    // 对照：无 reasoning 时同样 input 也是 level 0
    assert_eq!(compaction_urgency(100000, None, None, 1_048_576), 0);

    // reasoning 不影响 urgency：只有 input 高才触发
    // input=900000, reasoning=140000 → 当前占用 = 900000
    // 900000/1,040,384 = 86.5% -> level 2（reasoning 不改变结果）
    assert_eq!(compaction_urgency(900000, None, Some(140000), 1_048_576), 2);
}

#[test]
fn test_compaction_urgency_with_both() {
    // cached 不扣除，reasoning 不累加
    // input=200000, cached=100000, reasoning=50000 → 当前占用 = 200000
    // 200000/1,040,384 = 19.2% -> level 0
    assert_eq!(
        compaction_urgency(200000, Some(100000), Some(50000), 1_048_576),
        0
    );

    // input=900000, cached=200000, reasoning=140000 → 当前占用 = 900000
    // 900000/1,040,384 = 86.5% -> level 2（不再是 level 3）
    assert_eq!(
        compaction_urgency(900000, Some(200000), Some(140000), 1_048_576),
        2
    );

    // input=940000, cached=200000, reasoning=40000 → 当前占用 = 940000
    // 940000/1,040,384 = 90.4% -> level 3
    assert_eq!(
        compaction_urgency(940000, Some(200000), Some(40000), 1_048_576),
        3
    );
}

#[test]
fn test_compaction_urgency_cached_greater_than_input() {
    // cached_tokens 大于 input_tokens（不扣除），reasoning 也不累加
    // input=10000, cached=50000 (不扣除), reasoning=100000 → 当前占用 = 10000
    // 10000/1,040,384 = 0.96% -> level 0
    assert_eq!(
        compaction_urgency(10000, Some(50000), Some(100000), 1_048_576),
        0
    );
}
