use super::{reasoning_level_from_options, LlmClient, LlmConfigOptions, ReasoningConfig};
use crate::ReasoningLevel;

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
