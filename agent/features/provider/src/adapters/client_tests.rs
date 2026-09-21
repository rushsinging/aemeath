use super::{reasoning_level_from_options, LlmClient, LlmConfigOptions, ReasoningConfig};
use crate::adapters::pool::TransportPool;
use crate::ReasoningLevel;

fn pooled_config(model: &str, max_tokens: u32, base_url: Option<&str>) -> LlmConfigOptions {
    LlmConfigOptions {
        driver: "anthropic".to_string(),
        source_key: "anthropic".to_string(),
        api_style: None,
        api_key: "test-api-key".to_string(),
        base_url: base_url.map(str::to_string),
        model: model.to_string(),
        max_tokens,
        reasoning: false,
        reasoning_config: None,
        timeout_secs: 300,
        user_agent: Some("aemeath/test".to_string()),
    }
}

#[test]
fn from_config_with_pool_reuses_transport_across_model_switch() {
    let pool = TransportPool::new();
    let first = LlmClient::from_config_with_pool(
        pooled_config("claude-a", 8192, Some("https://api.anthropic.com")),
        &pool,
    )
    .expect("first client must build");
    let second = LlmClient::from_config_with_pool(
        pooled_config("claude-b", 16384, Some("https://api.anthropic.com")),
        &pool,
    )
    .expect("second client must build");

    assert_eq!(
        first.transport_id(),
        second.transport_id(),
        "model/max_tokens are invocation facts and must not rebuild the transport"
    );
}

#[test]
fn from_config_with_pool_builds_distinct_transport_for_distinct_endpoint() {
    let pool = TransportPool::new();
    let first = LlmClient::from_config_with_pool(
        pooled_config("claude-a", 8192, Some("https://api.anthropic.com")),
        &pool,
    )
    .expect("first client must build");
    let second = LlmClient::from_config_with_pool(
        pooled_config("claude-a", 8192, Some("https://proxy.example.com")),
        &pool,
    )
    .expect("second client must build");

    assert_ne!(
        first.transport_id(),
        second.transport_id(),
        "endpoint change must build a distinct transport"
    );
}

#[test]
fn thinking_budget_only_controls_disabled_or_enabled_fallback_level() {
    assert_eq!(
        reasoning_level_from_options(false, Some(&ReasoningConfig::ThinkingBudget(0))),
        ReasoningLevel::Off
    );
    assert_eq!(
        reasoning_level_from_options(false, Some(&ReasoningConfig::ThinkingBudget(1))),
        ReasoningLevel::High
    );
    assert_eq!(
        reasoning_level_from_options(false, Some(&ReasoningConfig::ThinkingBudget(40_000))),
        ReasoningLevel::High
    );
}
