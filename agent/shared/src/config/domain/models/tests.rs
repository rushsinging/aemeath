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
                reasoning: Some(false),
                reasoning_effort: None,
                api_style: None,
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
fn test_model_entry_reasoning_bool_true() {
    let json = r#"{"id":"glm-5.1","reasoning":true}"#;
    let entry: ModelEntryConfig = serde_json::from_str(json).unwrap();
    assert_eq!(entry.reasoning, Some(true));
}

#[test]
fn test_model_entry_reasoning_bool_false() {
    let json = r#"{"id":"gpt-5.5","reasoning":false}"#;
    let entry: ModelEntryConfig = serde_json::from_str(json).unwrap();
    assert_eq!(entry.reasoning, Some(false));
}

#[test]
fn test_model_entry_reasoning_absent() {
    let json = r#"{"id":"gpt-4o","name":"GPT-4o"}"#;
    let entry: ModelEntryConfig = serde_json::from_str(json).unwrap();
    assert!(entry.reasoning.is_none());
}

#[test]
fn test_model_entry_camel_case_token_aliases_deserialize() {
    let json = r#"{"id":"claude-sonnet-4-6","maxTokens":8192}"#;
    let entry: ModelEntryConfig = serde_json::from_str(json).unwrap();
    assert_eq!(entry.max_tokens, 8192);
}

#[path = "resolve_tests.rs"]
mod resolve_tests;
