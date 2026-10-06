use super::{reasoning_level_from_options, LlmClient, LlmConfigOptionsData, ReasoningConfig};
use crate::adapters::pool::TransportPool;
use crate::domain::capability::ReasoningLevel;

fn pooled_config(model: &str, max_tokens: u32, base_url: Option<&str>) -> LlmConfigOptionsData {
    LlmConfigOptionsData {
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
    let error = match LlmClient::from_config(LlmConfigOptionsData {
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
    let error = match LlmClient::from_config(LlmConfigOptionsData {
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
    let error = match LlmClient::from_config(LlmConfigOptionsData {
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

/// wire_provider_client 是组合根获得 provider 客户端的唯一装配入口：
/// 收编 from_config_with_pool + with_default_reasoning 装配链。
#[test]
fn wire_provider_client_applies_default_reasoning_over_pooled_transport() {
    let pool = TransportPool::new();
    let pooled = LlmClient::from_config_with_pool(
        pooled_config("claude-a", 8192, Some("https://api.anthropic.com")),
        &pool,
    )
    .expect("manual assembly baseline must build");

    let wired = super::wire_provider_client(
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
fn wire_provider_client_maps_configuration_failures_to_provider_error() {
    let pool = TransportPool::new();
    let mut config = pooled_config("claude-a", 16, None);
    config.driver = "not-a-real-driver".to_string();

    let error = match super::wire_provider_client(config, &pool, ReasoningLevel::Off) {
        Err(error) => error,
        Ok(_) => panic!("unknown driver must fail"),
    };
    assert_eq!(error.kind, crate::ProviderErrorKind::Configuration);
}

/// wire_provider_assembly 是 factory build 的 provider 侧装配内核：
/// config + 模型元数据 → (client, capability, 生效推理档位)，
/// capability 的 supports_*/reasoning/limits 构造全部收编于此。
#[test]
fn wire_provider_assembly_builds_capability_from_client_and_model_meta() {
    use crate::published_language::ModelIdData;
    use crate::published_language::ReasoningMappingKindData;

    let pool = TransportPool::new();
    let assembly = super::wire_provider_assembly(
        pooled_config("claude-a", 8192, Some("https://api.anthropic.com")),
        ModelIdData {
            provider: "Anthropic".to_string(),
            model: "claude-a".to_string(),
        },
        &pool,
        ReasoningLevel::Medium,
        Some(200_000),
        8192,
    )
    .expect("assembly must build");

    assert_eq!(assembly.requested_reasoning, ReasoningLevel::Medium);
    assert!(assembly.capability.supports_tools);
    assert!(assembly.capability.supports_streaming);
    assert_eq!(assembly.capability.context_limit, Some(200_000));
    assert_eq!(assembly.capability.output_limit, Some(8192));
    assert!(assembly
        .capability
        .reasoning
        .supported()
        .contains(&ReasoningLevel::Medium));
    assert_eq!(
        assembly.capability.reasoning.mapping,
        ReasoningMappingKindData::Effort
    );
    assert_eq!(assembly.client.model_name(), "claude-a");
}

#[test]
fn wire_provider_assembly_maps_config_failures_to_provider_error() {
    use crate::published_language::ModelIdData;

    let pool = TransportPool::new();
    let mut config = pooled_config("claude-a", 16, None);
    config.driver = "not-a-real-driver".to_string();

    let error = match super::wire_provider_assembly(
        config,
        ModelIdData {
            provider: "Anthropic".to_string(),
            model: "claude-a".to_string(),
        },
        &pool,
        ReasoningLevel::Off,
        Some(200_000),
        16,
    ) {
        Err(error) => error,
        Ok(_) => panic!("unknown driver must fail"),
    };
    assert_eq!(error.kind, crate::ProviderErrorKind::Configuration);
}
