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
fn test_needs_compaction_actual_with_cached_tokens() {
    // 有 cached_tokens，应该扣除
    // input=100000, cached=80000, actual_input=20000
    assert!(!needs_compaction_actual(
        100000,
        10000,
        Some(80000),
        None,
        1_048_576
    ));

    // input=100000, cached=0, actual_input=100000
    assert!(!needs_compaction_actual(
        100000,
        10000,
        Some(0),
        None,
        1_048_576
    ));
}

#[test]
fn test_needs_compaction_actual_with_reasoning_tokens() {
    // 有 reasoning_tokens，应该加上
    // input=50000, output=10000, reasoning=100000, total=160000
    assert!(!needs_compaction_actual(
        50000,
        10000,
        None,
        Some(100000),
        1_048_576
    ));

    // input=50000, output=10000, reasoning=950000, total=1010000
    // threshold = 1,027,384, 1010000 < threshold -> false
    assert!(!needs_compaction_actual(
        50000,
        10000,
        None,
        Some(950000),
        1_048_576
    ));

    // input=50000, output=10000, reasoning=970000, total=1030000
    // threshold = 1,027,384, 1030000 > threshold -> true
    assert!(needs_compaction_actual(
        50000,
        10000,
        None,
        Some(970000),
        1_048_576
    ));
}

#[test]
fn test_needs_compaction_actual_with_both() {
    // 同时有 cached_tokens 和 reasoning_tokens
    // input=200000, cached=150000, actual_input=50000
    // output=10000, reasoning=100000, total=160000
    assert!(!needs_compaction_actual(
        200000,
        10000,
        Some(150000),
        Some(100000),
        1_048_576
    ));

    // input=200000, cached=150000, actual_input=50000
    // output=10000, reasoning=970000, total=1030000
    // threshold = 1,027,384, 1030000 > threshold -> true
    assert!(needs_compaction_actual(
        200000,
        10000,
        Some(150000),
        Some(970000),
        1_048_576
    ));
}

#[test]
fn test_needs_compaction_actual_cached_greater_than_input() {
    // cached_tokens 大于 input_tokens，应该返回 0（不会 underflow）
    // input=10000, cached=50000, actual_input=0
    // output=10000, total=10000
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
    // 有 cached_tokens，应该扣除
    // input=100000, cached=80000, actual_input=20000, 20000/1,040,384 = 1.9% -> level 0
    assert_eq!(compaction_urgency(100000, Some(80000), None, 1_048_576), 0);

    // input=900000, cached=200000, actual_input=700000, 700000/1,040,384 = 67.3% -> level 0
    assert_eq!(compaction_urgency(900000, Some(200000), None, 1_048_576), 0);

    // input=900000, cached=100000, actual_input=800000, 800000/1,040,384 = 76.9% -> level 1
    assert_eq!(compaction_urgency(900000, Some(100000), None, 1_048_576), 1);
}

#[test]
fn test_compaction_urgency_with_reasoning_tokens() {
    // 有 reasoning_tokens，应该加上
    // input=100000, reasoning=50000, total=150000, 150000/1,040,384 = 14.4% -> level 0
    assert_eq!(compaction_urgency(100000, None, Some(50000), 1_048_576), 0);

    // input=100000, reasoning=630000, total=730000, 730000/1,040,384 = 70.2% -> level 1
    assert_eq!(compaction_urgency(100000, None, Some(630000), 1_048_576), 1);

    // input=100000, reasoning=740000, total=840000, 840000/1,040,384 = 80.7% -> level 2
    assert_eq!(compaction_urgency(100000, None, Some(740000), 1_048_576), 2);
}

#[test]
fn test_compaction_urgency_with_both() {
    // 同时有 cached_tokens 和 reasoning_tokens
    // input=200000, cached=100000, actual_input=100000
    // reasoning=50000, total=150000, 150000/1,040,384 = 14.4% -> level 0
    assert_eq!(
        compaction_urgency(200000, Some(100000), Some(50000), 1_048_576),
        0
    );

    // input=900000, cached=200000, actual_input=700000
    // reasoning=140000, total=840000, 840000/1,040,384 = 80.7% -> level 2
    assert_eq!(
        compaction_urgency(900000, Some(200000), Some(140000), 1_048_576),
        2
    );

    // input=900000, cached=200000, actual_input=700000
    // reasoning=40000, total=740000, 740000/1,040,384 = 71.1% -> level 1
    assert_eq!(
        compaction_urgency(900000, Some(200000), Some(40000), 1_048_576),
        1
    );
}

#[test]
fn test_compaction_urgency_cached_greater_than_input() {
    // cached_tokens 大于 input_tokens，actual_input=0
    // input=10000, cached=50000, actual_input=0
    // reasoning=100000, total=100000, 10% -> level 0
    assert_eq!(
        compaction_urgency(10000, Some(50000), Some(100000), 1_048_576),
        0
    );
}
