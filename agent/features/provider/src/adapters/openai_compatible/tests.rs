include!("tests/common.rs");
include!("tests/reasoning.rs");
include!("tests/provider_config.rs");
include!("tests/clamp_effort.rs");

#[test]
fn chat_usage_prefers_reported_total_without_double_counting_cache() {
    let usage = super::usage::parse_chat_usage(&serde_json::json!({
        "prompt_tokens": 100,
        "completion_tokens": 20,
        "total_tokens": 150,
        "prompt_tokens_details": {"cached_tokens": 80},
        "completion_tokens_details": {"reasoning_tokens": 5}
    }));

    assert_eq!(usage.cached_tokens, Some(80));
    assert_eq!(usage.reasoning_tokens, Some(5));
    assert_eq!(usage.total_tokens, Some(150));
}

#[test]
fn responses_usage_falls_back_to_input_plus_output_when_total_missing() {
    let usage = super::usage::parse_responses_usage(&serde_json::json!({
        "input_tokens": 100,
        "output_tokens": 20,
        "input_tokens_details": {"cached_tokens": 80},
        "output_tokens_details": {"reasoning_tokens": 5}
    }));

    assert_eq!(usage.cached_tokens, Some(80));
    assert_eq!(usage.reasoning_tokens, Some(5));
    assert_eq!(usage.total_tokens, Some(120));
}
