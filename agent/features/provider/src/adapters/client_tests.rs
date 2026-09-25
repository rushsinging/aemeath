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
fn from_config_rejects_missing_endpoint_instead_of_using_adapter_default() {
    let error = match LlmClient::from_config(LlmConfigOptions {
        driver: "anthropic".to_string(),
        source_key: "Anthropic".to_string(),
        api_style: None,
        api_key: String::new(),
        base_url: None,
        model: "claude-sonnet-5".to_string(),
        max_tokens: 128_000,
        reasoning: false,
        reasoning_config: None,
        timeout_secs: 30,
        user_agent: Some("test-agent".to_string()),
    }) {
        Ok(_) => panic!("Provider adapter 禁止为缺失 endpoint 自行 fallback"),
        Err(error) => error,
    };

    assert!(matches!(error, crate::LlmError::Config(_)));
    assert!(error.to_string().contains("base URL"));
}

#[test]
fn from_config_rejects_blank_model_instead_of_using_adapter_default() {
    let error = match LlmClient::from_config(LlmConfigOptions {
        driver: "openai".to_string(),
        source_key: "OpenAI".to_string(),
        api_style: None,
        api_key: String::new(),
        base_url: Some("https://api.openai.com".to_string()),
        model: "   ".to_string(),
        max_tokens: 16_384,
        reasoning: false,
        reasoning_config: None,
        timeout_secs: 30,
        user_agent: Some("test-agent".to_string()),
    }) {
        Ok(_) => panic!("Provider adapter 禁止为缺失模型自行 fallback"),
        Err(error) => error,
    };

    assert!(matches!(error, crate::LlmError::Config(_)));
    assert!(error.to_string().contains("模型"));
}

#[test]
fn from_config_rejects_missing_user_agent_instead_of_using_global_default() {
    let error = match LlmClient::from_config(LlmConfigOptions {
        driver: "openai".to_string(),
        source_key: "OpenAI".to_string(),
        api_style: None,
        api_key: String::new(),
        base_url: Some("https://api.openai.com".to_string()),
        model: "gpt-4o".to_string(),
        max_tokens: 16_384,
        reasoning: false,
        reasoning_config: None,
        timeout_secs: 30,
        user_agent: None,
    }) {
        Ok(_) => panic!("Provider adapter 禁止为缺失 UA 自行 fallback"),
        Err(error) => error,
    };

    assert!(matches!(error, crate::LlmError::Config(_)));
    assert!(error.to_string().contains("User-Agent"));
}

#[test]
#[should_panic(expected = "Provider construction 必须传入已解析 base URL")]
fn legacy_provider_constructor_no_longer_supplies_endpoint_fallback() {
    let _ = LlmClient::new(String::new());
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
