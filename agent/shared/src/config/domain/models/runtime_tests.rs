use super::*;
use crate::config::models::{ModelEntryConfig, ProviderModelsConfig};
use std::collections::HashMap;

fn models_config(model_max_tokens: u32) -> ModelsConfig {
    let mut providers = HashMap::new();
    providers.insert(
        "zhipu".to_string(),
        ProviderModelsConfig {
            base_url: "https://zhipu.example.com".to_string(),
            api_key: "zhipu-key".to_string(),
            driver: "zhipu".to_string(),
            models: vec![ModelEntryConfig {
                id: "glm-5.2".to_string(),
                name: "GLM 5.2".to_string(),
                context_window: 128_000,
                max_tokens: model_max_tokens,
                reasoning: None,
                reasoning_effort: None,
                api_style: None,
                input: Vec::new(),
            }],
            user_agent: None,
        },
    );
    ModelsConfig {
        mode: String::new(),
        default: "zhipu/glm-5.2".to_string(),
        providers,
        guidance: HashMap::new(),
    }
}

fn resolve(
    cli_max_tokens: Option<u32>,
    model_max_tokens: u32,
    config_max_tokens: Option<u32>,
) -> Result<ResolvedRuntimeModel, RuntimeModelResolutionError> {
    RuntimeModelResolver::resolve(
        &models_config(model_max_tokens),
        RuntimeModelRequest {
            model_override: None,
            cli_max_tokens,
            config_max_tokens,
        },
    )
}

#[test]
fn test_runtime_model_resolver_cli_wins_over_model_and_config() {
    let result = resolve(Some(4096), 8192, Some(200_000)).unwrap();

    assert_eq!(result.max_tokens(), 4096);
    assert_eq!(result.max_tokens_source(), MaxTokensSource::Cli);
}

#[test]
fn test_runtime_model_resolver_cli_large_value_is_allowed() {
    let result = resolve(Some(200_000), 8192, Some(4096)).unwrap();

    assert_eq!(result.max_tokens(), 200_000);
    assert_eq!(result.max_tokens_source(), MaxTokensSource::Cli);
}

#[test]
fn test_runtime_model_resolver_cli_zero_errors() {
    let err = resolve(Some(0), 8192, Some(4096)).unwrap_err();

    assert_eq!(err, RuntimeModelResolutionError::CliMaxTokensZero);
}

#[test]
fn test_runtime_model_resolver_model_wins_over_config() {
    let result = resolve(None, 8192, Some(200_000)).unwrap();

    assert_eq!(result.max_tokens(), 8192);
    assert_eq!(result.max_tokens_source(), MaxTokensSource::Model);
}

#[test]
fn test_runtime_model_resolver_model_large_value_is_allowed() {
    let result = resolve(None, 200_000, Some(8192)).unwrap();

    assert_eq!(result.max_tokens(), 200_000);
    assert_eq!(result.max_tokens_source(), MaxTokensSource::Model);
}

#[test]
fn test_runtime_model_resolver_model_zero_uses_config() {
    let result = resolve(None, 0, Some(200_000)).unwrap();

    assert_eq!(result.max_tokens(), 200_000);
    assert_eq!(result.max_tokens_source(), MaxTokensSource::Config);
}

#[test]
fn test_runtime_model_resolver_config_zero_uses_default() {
    let result = resolve(None, 0, Some(0)).unwrap();

    assert_eq!(result.max_tokens(), DEFAULT_MAX_TOKENS);
    assert_eq!(result.max_tokens_source(), MaxTokensSource::Default);
}

#[test]
fn test_runtime_model_resolver_missing_values_use_default() {
    let result = resolve(None, 0, None).unwrap();

    assert_eq!(result.max_tokens(), DEFAULT_MAX_TOKENS);
    assert_eq!(result.max_tokens_source(), MaxTokensSource::Default);
}

#[test]
fn test_runtime_model_resolver_model_override_selects_model() {
    let config = models_config(16_000);
    let result = RuntimeModelResolver::resolve(
        &config,
        RuntimeModelRequest {
            model_override: Some("zhipu/glm-5.2"),
            cli_max_tokens: None,
            config_max_tokens: Some(8192),
        },
    )
    .unwrap();

    assert_eq!(result.resolved_model().source_key, "zhipu");
    assert_eq!(result.resolved_model().model.id, "glm-5.2");
    assert_eq!(result.max_tokens(), 16_000);
}

#[test]
fn test_runtime_model_resolver_unknown_model_returns_model_error() {
    let config = models_config(16_000);
    let err = RuntimeModelResolver::resolve(
        &config,
        RuntimeModelRequest {
            model_override: Some("zhipu/missing"),
            cli_max_tokens: None,
            config_max_tokens: Some(8192),
        },
    )
    .unwrap_err();

    assert!(matches!(err, RuntimeModelResolutionError::Model(_)));
}
