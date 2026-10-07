use super::{reasoning_level_from_options, LlmClient, ProviderClientSpecData, ReasoningConfig};
use crate::adapters::pool::TransportPool;
use crate::domain::capability::ReasoningLevel;

fn pooled_config(model: &str, max_tokens: u32, base_url: Option<&str>) -> ProviderClientSpecData {
    ProviderClientSpecData {
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
    let error = match LlmClient::from_config(ProviderClientSpecData {
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

    assert_eq!(error.kind, crate::ProviderErrorKind::Configuration);
    assert!(error.to_string().contains("base URL"));
}

#[test]
fn from_config_rejects_blank_model_instead_of_using_adapter_default() {
    let error = match LlmClient::from_config(ProviderClientSpecData {
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

    assert_eq!(error.kind, crate::ProviderErrorKind::Configuration);
    assert!(error.to_string().contains("模型"));
}

#[test]
fn from_config_rejects_missing_user_agent_instead_of_using_global_default() {
    let error = match LlmClient::from_config(ProviderClientSpecData {
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

    assert_eq!(error.kind, crate::ProviderErrorKind::Configuration);
    assert!(error.to_string().contains("User-Agent"));
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

/// assemble_client 是 wire_provider_client 的装配内核：
/// 收编 from_config_with_pool + with_default_reasoning 装配链。
#[test]
fn assemble_client_applies_default_reasoning_over_pooled_transport() {
    let pool = TransportPool::new();
    let pooled = LlmClient::from_config_with_pool(
        pooled_config("claude-a", 8192, Some("https://api.anthropic.com")),
        &pool,
    )
    .expect("manual assembly baseline must build");

    let wired = super::assemble_client(
        pooled_config("claude-a", 8192, Some("https://api.anthropic.com")),
        &pool,
        ReasoningLevel::Medium,
    )
    .expect("wire assembly must build");

    assert_eq!(
        wired.transport_id(),
        pooled.transport_id(),
        "wire 装配与手工装配共享同一 transport pool 语义"
    );
    assert_eq!(
        wired.default_scope().requested_reasoning(),
        ReasoningLevel::Medium
    );
}

#[test]
fn assemble_client_maps_configuration_failures_to_provider_error() {
    let pool = TransportPool::new();
    let mut config = pooled_config("claude-a", 16, None);
    config.driver = "not-a-real-driver".to_string();

    let error = match super::assemble_client(config, &pool, ReasoningLevel::Off) {
        Err(error) => error,
        Ok(_) => panic!("unknown driver must fail"),
    };
    assert_eq!(error.kind, crate::ProviderErrorKind::Configuration);
}

/// wire_provider_client 是 factory build 的 provider 侧装配单入口：
/// config + 模型元数据 → (client, 修正版 ModelInfo)；
/// supported_reasoning 阶梯由 client 推导覆盖，身份/supports_*/limits 来自组合根投影。
#[test]
fn wire_provider_client_builds_model_info_from_client_and_model_meta() {
    use crate::published_language::ModelInfo;

    let pool = TransportPool::new();
    let (client, model) = super::wire_provider_client(
        pooled_config("claude-a", 8192, Some("https://api.anthropic.com")),
        ModelInfo {
            provider: "Anthropic".to_string(),
            model: "claude-a".to_string(),
            supports_tools: true,
            supports_parallel_tool_calls: true,
            supports_streaming: true,
            supported_reasoning: vec![ReasoningLevel::Off],
            context_limit: Some(200_000),
            output_limit: Some(8192),
        },
        &pool,
    )
    .expect("assembly must build");

    assert!(model.supports_tools);
    assert!(model.supports_streaming);
    assert_eq!(model.context_limit, Some(200_000));
    assert_eq!(model.output_limit, Some(8192));
    assert!(model.supported_reasoning.contains(&ReasoningLevel::Medium));
    assert_eq!(
        model.resolve_reasoning(ReasoningLevel::Medium),
        ReasoningLevel::Medium,
        "anthropic 阶梯覆盖占位后必须能解析 Medium"
    );
    assert_eq!(client.model_name(), "claude-a");
}

#[test]
fn wire_provider_client_maps_config_failures_to_provider_error() {
    use crate::published_language::ModelInfo;

    let pool = TransportPool::new();
    let mut config = pooled_config("claude-a", 16, None);
    config.driver = "not-a-real-driver".to_string();

    let error = match super::wire_provider_client(
        config,
        ModelInfo {
            provider: "Anthropic".to_string(),
            model: "claude-a".to_string(),
            supports_tools: true,
            supports_parallel_tool_calls: true,
            supports_streaming: true,
            supported_reasoning: vec![ReasoningLevel::Off],
            context_limit: Some(200_000),
            output_limit: Some(16),
        },
        &pool,
    ) {
        Err(error) => error,
        Ok(_) => panic!("unknown driver must fail"),
    };
    assert_eq!(error.kind, crate::ProviderErrorKind::Configuration);
}
