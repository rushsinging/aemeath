//! `ProviderModelsConfig::user_agent` 字段的归一化与往返行为契约测试。
//!
//! 详见 `types.rs` 中相应字段的设计：
//! - JSON 字段名固定为 `userAgent`；
//! - 缺失字段视为未配置（向后兼容旧配置文件）；
//! - 空白字符串在归一化函数下回退为 `None`，不进入回退链。

use super::*;
use std::collections::HashMap;

fn provider_with_user_agent(value: &str) -> ProviderModelsConfig {
    ProviderModelsConfig {
        base_url: "https://example.test".to_string(),
        api_key: String::new(),
        driver: "anthropic".to_string(),
        models: vec![ModelEntryConfig {
            id: "claude-sonnet-4-5".to_string(),
            ..Default::default()
        }],
        user_agent: Some(value.to_string()),
    }
}

#[test]
fn user_agent_missing_in_legacy_json_deserializes_as_none() {
    // 旧配置文件不包含 `userAgent` 字段，必须保持兼容。
    let json = r#"{
        "baseUrl": "https://example.test",
        "apiKey": "",
        "driver": "anthropic",
        "models": [{"id": "claude-sonnet-4-5"}]
    }"#;

    let provider: ProviderModelsConfig =
        serde_json::from_str(json).expect("缺失 userAgent 的旧 JSON 必须反序列化成功以保持兼容");

    assert!(
        provider.user_agent.is_none(),
        "缺失 userAgent 字段必须视为未配置，得到 None"
    );
}

#[test]
fn user_agent_explicit_value_round_trips_via_user_agent_key() {
    let provider = provider_with_user_agent("claude-cli/1.0.0");

    let serialized = serde_json::to_string(&provider).expect("序列化 ProviderModelsConfig 成功");
    assert!(
        serialized.contains(r#""userAgent":"claude-cli/1.0.0""#),
        "序列化的 JSON 必须包含 userAgent camelCase 字段，得到 {serialized}"
    );

    let restored: ProviderModelsConfig =
        serde_json::from_str(&serialized).expect("包含 userAgent 的 JSON 反序列化成功");
    assert_eq!(restored.user_agent.as_deref(), Some("claude-cli/1.0.0"));
}

#[test]
fn user_agent_blank_value_round_trips_but_normalizes_to_none() {
    let provider = provider_with_user_agent("   \t\n  ");

    let serialized = serde_json::to_string(&provider).expect("序列化 ProviderModelsConfig 成功");
    // 序列化为 Some 时必须保留原始字符串。
    assert!(
        serialized.contains(r#""userAgent""#),
        "Some 字段必须保留原始字符串（即使全空白）"
    );

    assert!(
        provider.normalized_user_agent().is_none(),
        "全空白 userAgent 在归一化后必须视为 None（不可作为 HeaderValue 安全使用）"
    );
}

#[test]
fn user_agent_missing_normalizes_to_none() {
    let provider = ProviderModelsConfig::default();
    assert!(
        provider.normalized_user_agent().is_none(),
        "缺失 userAgent 字段归一化后必须为 None"
    );
}

#[test]
fn user_agent_non_blank_normalizes_to_trimmed_inner_value() {
    let provider = provider_with_user_agent("  claude-cli/1.0.0  ");

    assert_eq!(
        provider.normalized_user_agent(),
        Some("claude-cli/1.0.0"),
        "非空白 userAgent 必须去除首尾空白后返回原始字符串"
    );
}

#[test]
fn user_agent_with_normalize_produces_clone_with_blank_cleared() {
    let provider = provider_with_user_agent("   ");
    let normalized = provider.with_normalized_user_agent();

    assert!(
        normalized.user_agent.is_none(),
        "with_normalized_user_agent 必须把空白归一为 None，得到 {:?}",
        normalized.user_agent
    );
    // 其它字段保持不变。
    assert_eq!(normalized.driver, "anthropic");
    assert_eq!(normalized.base_url, "https://example.test");
}

#[test]
fn user_agent_with_normalize_keeps_non_blank_value_unchanged() {
    let provider = provider_with_user_agent("claude-cli/1.0.0");
    let normalized = provider.with_normalized_user_agent();

    assert_eq!(
        normalized.user_agent.as_deref(),
        Some("claude-cli/1.0.0"),
        "with_normalized_user_agent 对非空白值必须保持不变"
    );
}

#[test]
fn user_agent_field_is_omitted_from_json_when_absent() {
    // 验证 `skip_serializing_if = "Option::is_none"` 生效。
    let provider = ProviderModelsConfig::default();
    let serialized = serde_json::to_string(&provider).expect("序列化 ProviderModelsConfig 成功");
    assert!(
        !serialized.contains("userAgent"),
        "None userAgent 不得写入 JSON，得到 {serialized}"
    );
}

#[test]
fn user_agent_unknown_fields_in_provider_models_config_are_still_tolerated() {
    // ProviderModelsConfig 顶层结构允许向前兼容；新增字段不应破坏现有配置文件。
    let json = r#"{
        "baseUrl": "https://example.test",
        "driver": "anthropic",
        "models": [],
        "userAgent": "claude-cli/1.0.0",
        "futureUnknownField": true
    }"#;

    let provider: ProviderModelsConfig =
        serde_json::from_str(json).expect("含未知字段的 JSON 也应反序列化成功以保持向前兼容");
    assert_eq!(provider.user_agent.as_deref(), Some("claude-cli/1.0.0"));

    // 同时确保 ModelsConfig 仍然可以承载 ProviderModelsConfig。
    let mut providers = HashMap::new();
    providers.insert("anthropic".to_string(), provider);
    let config = ModelsConfig {
        mode: String::new(),
        default: String::new(),
        providers,
        guidance: HashMap::new(),
    };
    let round_tripped = serde_json::to_string(&config).expect("ModelsConfig 序列化成功");
    let parsed: ModelsConfig =
        serde_json::from_str(&round_tripped).expect("ModelsConfig 反序列化成功");
    assert_eq!(parsed.providers.len(), 1);
    assert_eq!(
        parsed
            .providers
            .get("anthropic")
            .and_then(|p| p.user_agent.as_deref()),
        Some("claude-cli/1.0.0")
    );
}
