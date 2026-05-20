use super::*;
use crate::config::models::resolve::normalize_model_key;
use crate::config::{Config, ConfigManager};
use crate::provider::ApiDriverKind;
use std::collections::HashMap;

fn test_config() -> ModelsConfig {
    let mut providers = HashMap::new();
    providers.insert(
        "LiteLLM".to_string(),
        ProviderModelsConfig {
            base_url: "http://localhost:4000".to_string(),
            api_key: String::new(),
            api: "openai".to_string(),
            models: vec![ModelEntryConfig {
                id: "gpt-5.5".to_string(),
                name: "GPT-5.5".to_string(),
                input: vec!["text".to_string()],
                context_window: 200_000,
                max_tokens: 32_000,
                thinking_max_tokens: 0,
                reasoning: Some(false),
                reasoning_effort: None,
            }],
        },
    );
    ModelsConfig {
        mode: String::new(),
        default: String::new(),
        providers,
        guidance: HashMap::new(),
    }
}

#[test]
fn test_default_models_config_includes_volcengine_coding_plan() {
    let config = volcengine_coding_plan_config();
    let provider = config.providers.get("Volcengine").unwrap();

    assert_eq!(
        config.default,
        "Volcengine/doubao-seed-2-0-code-preview-260215"
    );
    assert_eq!(provider.api, "volcengine");
    assert_eq!(
        provider.base_url,
        "https://ark.cn-beijing.volces.com/api/coding/v3"
    );
    assert!(provider.api_key.is_empty());
    assert!(provider
        .models
        .iter()
        .any(|m| m.id == "doubao-seed-2-0-code-preview-260215"));
}

#[test]
fn test_default_models_config_keeps_latest_vendor_models() {
    let config = volcengine_coding_plan_config();
    let provider = config.providers.get("Volcengine").unwrap();
    let ids: Vec<&str> = provider.models.iter().map(|m| m.id.as_str()).collect();

    assert!(ids.contains(&"glm-4-7-251222"));
    assert!(ids.contains(&"deepseek-v3-2-251201"));
    assert!(ids.contains(&"kimi-k2-thinking-251104"));
    assert!(ids.contains(&"doubao-seed-2-0-pro-260215"));
    assert!(ids.contains(&"doubao-seed-2-0-lite-260428"));
    assert!(ids.contains(&"doubao-seed-2-0-mini-260428"));
    assert!(!ids.iter().any(|id| id.contains("minimax")));
}

#[test]
fn test_default_models_config_resolves_volcengine_default() {
    let config = volcengine_coding_plan_config();
    let resolved = config.resolve_default_model().unwrap();

    assert_eq!(resolved.source_key, "Volcengine");
    assert_eq!(resolved.api, ApiDriverKind::Volcengine);
    assert_eq!(resolved.model.id, "doubao-seed-2-0-code-preview-260215");
    assert_eq!(resolved.model.reasoning, Some(true));
    assert_eq!(resolved.model.thinking_max_tokens, 131_072);
}

#[test]
fn test_empty_models_config_keeps_empty_default_semantics() {
    let config = ModelsConfig::default();

    assert!(config.default.is_empty());
    assert!(config.providers.is_empty());
}

#[test]
fn test_merge_config_keeps_volcengine_builtin_without_overriding_user_default() {
    let builtin = Config {
        models: volcengine_coding_plan_config(),
        ..Default::default()
    };
    let mut user = Config::default();
    user.models.default = "Custom/custom-model".to_string();

    let merged = ConfigManager::merge_config(builtin, user);

    assert_eq!(merged.models.default, "Custom/custom-model");
    assert!(merged.models.providers.contains_key("Volcengine"));
}

#[test]
fn test_find_model_exact_source_case_insensitive() {
    let config = test_config();
    let result = config.find_model("litellm/gpt-5.5");
    assert!(result.is_some());
    let (source, _, model) = result.unwrap();
    assert_eq!(source, "LiteLLM");
    assert_eq!(model.id, "gpt-5.5");
    assert_eq!(model.reasoning, Some(false));
}

#[test]
fn test_find_model_display_name_case_insensitive_source() {
    let config = test_config();
    let result = config.find_model("litellm/GPT-5.5");
    assert!(result.is_some());
    let (_, _, model) = result.unwrap();
    assert_eq!(model.name, "GPT-5.5");
}

#[test]
fn test_find_model_unknown_source_returns_none() {
    let config = test_config();
    let result = config.find_model("openai/gpt-5.5");
    assert!(result.is_none());
}

#[test]
fn test_validate_reasoning_effort_valid() {
    for valid in &["none", "low", "medium", "high", "xhigh"] {
        assert!(validate_reasoning_effort(valid).is_ok());
    }
}

#[test]
fn test_validate_reasoning_effort_invalid() {
    assert!(validate_reasoning_effort("turbo").is_err());
    assert!(validate_reasoning_effort("HIGH").is_err());
    assert!(validate_reasoning_effort("").is_err());
}

#[test]
fn test_supports_reasoning_effort() {
    assert!(supports_reasoning_effort("gpt-5.5"));
    assert!(supports_reasoning_effort("gpt-5"));
    assert!(supports_reasoning_effort("o1"));
    assert!(supports_reasoning_effort("o3-mini"));
    assert!(supports_reasoning_effort("o4-mini"));
    assert!(!supports_reasoning_effort("gpt-4o"));
    assert!(!supports_reasoning_effort("deepseek-r1"));
    assert!(!supports_reasoning_effort("claude-opus-4"));
}

#[test]
fn test_model_entry_reasoning_effort_deserialize() {
    let json = r#"{"id":"gpt-5.5","reasoning_effort":"low"}"#;
    let entry: ModelEntryConfig = serde_json::from_str(json).unwrap();
    assert_eq!(entry.reasoning_effort, Some("low".to_string()));
    assert_eq!(entry.id, "gpt-5.5");
}

#[test]
fn test_model_entry_reasoning_effort_default_none() {
    let json = r#"{"id":"gpt-4o"}"#;
    let entry: ModelEntryConfig = serde_json::from_str(json).unwrap();
    assert!(entry.reasoning_effort.is_none());
}

#[test]
fn test_model_entry_reasoning_object_effort() {
    let json = r#"{"id":"gpt-5.5","reasoning":{"effort":"medium"}}"#;
    let entry: ModelEntryConfig = serde_json::from_str(json).unwrap();
    assert_eq!(entry.reasoning, Some(true));
    assert_eq!(entry.reasoning_effort, Some("medium".to_string()));
}

#[test]
fn test_model_entry_reasoning_object_effort_field_wins() {
    let json = r#"{"id":"gpt-5.5","reasoning":{"effort":"low"},"reasoning_effort":"high"}"#;
    let entry: ModelEntryConfig = serde_json::from_str(json).unwrap();
    assert_eq!(entry.reasoning, Some(true));
    assert_eq!(entry.reasoning_effort, Some("high".to_string()));
}

#[test]
fn test_model_entry_reasoning_bool_true() {
    let json = r#"{"id":"glm-5.1","reasoning":true}"#;
    let entry: ModelEntryConfig = serde_json::from_str(json).unwrap();
    assert_eq!(entry.reasoning, Some(true));
    assert!(entry.reasoning_effort.is_none());
}

#[test]
fn test_model_entry_reasoning_bool_false() {
    let json = r#"{"id":"gpt-5.5","reasoning":false}"#;
    let entry: ModelEntryConfig = serde_json::from_str(json).unwrap();
    assert_eq!(entry.reasoning, Some(false));
    assert!(entry.reasoning_effort.is_none());
}

#[test]
fn test_model_entry_reasoning_absent() {
    let json = r#"{"id":"gpt-4o","name":"GPT-4o"}"#;
    let entry: ModelEntryConfig = serde_json::from_str(json).unwrap();
    assert!(entry.reasoning.is_none());
    assert!(entry.reasoning_effort.is_none());
}

#[test]
fn test_model_entry_thinking_max_tokens_deserialize() {
    let json = r#"{"id":"claude-sonnet-4-6","thinking_max_tokens":4096}"#;
    let entry: ModelEntryConfig = serde_json::from_str(json).unwrap();
    assert_eq!(entry.thinking_max_tokens, 4096);
}

#[test]
fn test_model_entry_camel_case_token_aliases_deserialize() {
    let json = r#"{"id":"claude-sonnet-4-6","maxTokens":8192,"thinkingMaxTokens":4096}"#;
    let entry: ModelEntryConfig = serde_json::from_str(json).unwrap();
    assert_eq!(entry.max_tokens, 8192);
    assert_eq!(entry.thinking_max_tokens, 4096);
}

#[test]
fn test_model_entry_thinking_max_tokens_default_zero() {
    let json = r#"{"id":"claude-sonnet-4-6"}"#;
    let entry: ModelEntryConfig = serde_json::from_str(json).unwrap();
    assert_eq!(entry.thinking_max_tokens, 0);
}

#[test]
fn test_full_config_with_reasoning_object() {
    let json = r#"{
        "models": {
            "providers": {
                "LiteLLM": {
                    "baseUrl": "https://litellm.example.com",
                    "api": "litellm",
                    "models": [
                        {
                            "id": "gpt-5.5",
                            "reasoning": { "effort": "medium" },
                            "contextWindow": 1000000,
                            "maxTokens": 128000
                        }
                    ]
                }
            }
        }
    }"#;
    use crate::config::Config;
    let config: Config = serde_json::from_str(json).unwrap();
    let resolved = config
        .models
        .resolve_model_selection("LiteLLM/gpt-5.5")
        .unwrap();
    assert_eq!(resolved.model.reasoning, Some(true));
    assert_eq!(resolved.model.reasoning_effort, Some("medium".to_string()));
}

fn resolver_config() -> ModelsConfig {
    let mut providers = HashMap::new();
    providers.insert(
        "Zhipu".to_string(),
        ProviderModelsConfig {
            base_url: "https://zhipu.example.com".to_string(),
            api_key: "zhipu-key".to_string(),
            api: "zhipu".to_string(),
            models: vec![ModelEntryConfig {
                id: "glm-5.1".to_string(),
                name: "GLM 5.1".to_string(),
                context_window: 128_000,
                max_tokens: 32_000,
                reasoning: Some(true),
                ..Default::default()
            }],
        },
    );
    providers.insert(
        "LiteLLM".to_string(),
        ProviderModelsConfig {
            base_url: "https://litellm.example.com".to_string(),
            api_key: "litellm-key".to_string(),
            api: "litellm".to_string(),
            models: vec![ModelEntryConfig {
                id: "anthropic/claude-opus-4-7".to_string(),
                name: "Claude via LiteLLM".to_string(),
                context_window: 200_000,
                max_tokens: 16_000,
                reasoning: None,
                ..Default::default()
            }],
        },
    );
    ModelsConfig {
        mode: String::new(),
        default: "Zhipu/glm-5.1".to_string(),
        providers,
        guidance: HashMap::new(),
    }
}

#[test]
fn test_resolve_model_selection_zhipu() {
    let config = resolver_config();
    let resolved = config.resolve_model_selection("zhipu/glm-5.1").unwrap();
    assert_eq!(resolved.source_key, "Zhipu");
    assert_eq!(resolved.model.id, "glm-5.1");
    assert_eq!(resolved.api, ApiDriverKind::Zhipu);
    assert_eq!(resolved.source_config.api, "zhipu");
}

#[test]
fn test_resolve_model_selection_litellm_model_id_with_slash() {
    let config = resolver_config();
    let resolved = config
        .resolve_model_selection("LiteLLM/anthropic/claude-opus-4-7")
        .unwrap();
    assert_eq!(resolved.source_key, "LiteLLM");
    assert_eq!(resolved.model.id, "anthropic/claude-opus-4-7");
    assert_eq!(resolved.api, ApiDriverKind::LiteLLM);
}

#[test]
fn test_resolve_model_selection_unknown_source_lists_available() {
    let config = resolver_config();
    let err = config
        .resolve_model_selection("Missing/glm-5.1")
        .unwrap_err();
    let message = err.to_string();
    assert!(message.contains("未找到模型来源 'Missing'"));
    assert!(message.contains("Zhipu"));
    assert!(message.contains("LiteLLM"));
}

#[test]
fn test_resolve_model_selection_unknown_model_lists_available() {
    let config = resolver_config();
    let err = config.resolve_model_selection("Zhipu/glm-x").unwrap_err();
    let message = err.to_string();
    assert!(message.contains("来源 'Zhipu' 中未找到模型 'glm-x'"));
    assert!(message.contains("glm-5.1"));
}

#[test]
fn test_resolve_model_selection_rejects_openai_compatible_api() {
    let mut config = resolver_config();
    let source = config.providers.get_mut("Zhipu").unwrap();
    source.api = "openai-compatible".to_string();

    let err = config.resolve_model_selection("Zhipu/glm-5.1").unwrap_err();

    assert_eq!(
        err,
        ModelResolveError::UnknownApi {
            source: "Zhipu".to_string(),
            api: "openai-compatible".to_string(),
        }
    );
}

#[test]
fn test_resolve_default_model_uses_config_default() {
    let config = resolver_config();
    let resolved = config.resolve_default_model().unwrap();
    assert_eq!(resolved.source_key, "Zhipu");
    assert_eq!(resolved.model.id, "glm-5.1");
}

#[test]
fn test_normalize_model_key() {
    assert_eq!(normalize_model_key("DeepSeek-V4-Pro ⚡"), "deepseek-v4-pro");
    assert_eq!(normalize_model_key("GPT-5.5"), "gpt-5.5");
    assert_eq!(normalize_model_key("GLM 5.1"), "glm5.1");
    assert_eq!(normalize_model_key(""), "");
}

#[test]
fn test_display_label() {
    let model = ModelEntryConfig {
        id: "glm-5.1".to_string(),
        name: "GLM 5.1 ⚡".to_string(),
        ..Default::default()
    };
    assert_eq!(model.display_label(), "GLM 5.1 ⚡ (id: glm-5.1)");

    let model2 = ModelEntryConfig {
        id: "gpt-5.5".to_string(),
        name: "".to_string(),
        ..Default::default()
    };
    assert_eq!(model2.display_label(), "gpt-5.5");

    let model3 = ModelEntryConfig {
        id: "gpt-5.5".to_string(),
        name: "gpt-5.5".to_string(),
        ..Default::default()
    };
    assert_eq!(model3.display_label(), "gpt-5.5");
}

#[test]
fn test_provider_ci() {
    let config = resolver_config();
    assert!(config.provider_ci("zhipu").is_some());
    assert!(config.provider_ci("ZHIPU").is_some());
    assert!(config.provider_ci("Zhipu").is_some());
    assert!(config.provider_ci("unknown").is_none());
}
