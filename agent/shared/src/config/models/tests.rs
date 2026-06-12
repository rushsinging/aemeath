use super::*;
use crate::config::models::resolve::normalize_model_key;
use std::collections::HashMap;

fn test_config() -> ModelsConfig {
    let mut providers = HashMap::new();
    providers.insert(
        "LiteLLM".to_string(),
        ProviderModelsConfig {
            base_url: "http://localhost:4000".to_string(),
            api_key: String::new(),
            driver: "openai".to_string(),
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
fn test_empty_models_config_keeps_empty_default_semantics() {
    let config = ModelsConfig::default();

    assert!(config.default.is_empty());
    assert!(config.providers.is_empty());
}

#[test]
fn test_find_model_exact_source_case_sensitive() {
    let config = test_config();
    let result = config.find_model("LiteLLM/gpt-5.5");
    assert!(result.is_some());
    let (source, _, model) = result.unwrap();
    assert_eq!(source, "LiteLLM");
    assert_eq!(model.id, "gpt-5.5");
    assert_eq!(model.reasoning, Some(false));
}

#[test]
fn test_find_model_rejects_source_case_mismatch() {
    let config = test_config();
    let result = config.find_model("litellm/gpt-5.5");
    assert!(result.is_none());
}

#[test]
fn test_find_model_display_name_with_exact_source_case() {
    let config = test_config();
    let result = config.find_model("LiteLLM/GPT-5.5");
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

#[path = "resolve_tests.rs"]
mod resolve_tests;
