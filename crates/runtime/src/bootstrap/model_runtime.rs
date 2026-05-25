use crate::api::core::config::models::ModelEntryConfig;
use crate::api::core::config::{models::validate_reasoning_effort, Config};

pub struct ModelRuntimeSettings {
    pub max_tokens: u32,
    pub thinking_max_tokens: u32,
    pub reasoning: bool,
    pub reasoning_effort: Option<String>,
}

pub struct ReasoningConfigInput {
    pub cli_reasoning_effort: Option<String>,
    pub env_reasoning_effort: Option<String>,
}

pub fn resolve_model_runtime_settings(
    cli_max_tokens: Option<u32>,
    model: &ModelEntryConfig,
    config_file: Option<&Config>,
    cli_reasoning_default: bool,
    reasoning_input: ReasoningConfigInput,
) -> Result<ModelRuntimeSettings, String> {
    let max_tokens = cli_max_tokens.unwrap_or_else(|| {
        if model.max_tokens > 0 {
            model.max_tokens
        } else if config_file
            .as_ref()
            .map(|config| config.model.max_tokens > 0)
            .unwrap_or(false)
        {
            config_file.as_ref().unwrap().model.max_tokens
        } else {
            32_000
        }
    });
    let thinking_max_tokens = model.thinking_max_tokens;
    let reasoning = model.reasoning.unwrap_or(cli_reasoning_default);
    let reasoning_effort = reasoning_input
        .cli_reasoning_effort
        .or_else(|| model.reasoning_effort.clone())
        .or(reasoning_input.env_reasoning_effort)
        .filter(|effort| !effort.is_empty());

    if let Some(ref effort) = reasoning_effort {
        validate_reasoning_effort(effort)?;
    }

    Ok(ModelRuntimeSettings {
        max_tokens,
        thinking_max_tokens,
        reasoning,
        reasoning_effort,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::core::config::models::ModelEntryConfig;
    use crate::api::core::config::{Config, ModelConfig};

    fn model_entry(max_tokens: u32) -> ModelEntryConfig {
        ModelEntryConfig {
            id: "model-id".to_string(),
            name: "model-name".to_string(),
            input: Vec::new(),
            context_window: 128_000,
            max_tokens,
            thinking_max_tokens: 4096,
            reasoning: None,
            reasoning_effort: None,
        }
    }

    fn config_with_max_tokens(max_tokens: u32) -> Config {
        Config {
            model: ModelConfig {
                max_tokens,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    fn reasoning_input(
        cli_reasoning_effort: Option<&str>,
        env_reasoning_effort: Option<&str>,
    ) -> ReasoningConfigInput {
        ReasoningConfigInput {
            cli_reasoning_effort: cli_reasoning_effort.map(str::to_string),
            env_reasoning_effort: env_reasoning_effort.map(str::to_string),
        }
    }

    #[test]
    fn test_resolve_model_runtime_settings_prefers_cli_max_tokens() {
        let model = model_entry(16_000);
        let config = config_with_max_tokens(24_000);

        let result = resolve_model_runtime_settings(
            Some(8_000),
            &model,
            Some(&config),
            true,
            reasoning_input(None, None),
        )
        .unwrap();

        assert_eq!(result.max_tokens, 8_000);
    }

    #[test]
    fn test_resolve_model_runtime_settings_uses_model_max_tokens() {
        let model = model_entry(16_000);
        let config = config_with_max_tokens(24_000);

        let result = resolve_model_runtime_settings(
            None,
            &model,
            Some(&config),
            true,
            reasoning_input(None, None),
        )
        .unwrap();

        assert_eq!(result.max_tokens, 16_000);
    }

    #[test]
    fn test_resolve_model_runtime_settings_uses_config_max_tokens_when_model_zero() {
        let model = model_entry(0);
        let config = config_with_max_tokens(24_000);

        let result = resolve_model_runtime_settings(
            None,
            &model,
            Some(&config),
            true,
            reasoning_input(None, None),
        )
        .unwrap();

        assert_eq!(result.max_tokens, 24_000);
    }

    #[test]
    fn test_resolve_model_runtime_settings_uses_default_max_tokens_when_missing() {
        let model = model_entry(0);
        let config = config_with_max_tokens(0);

        let result = resolve_model_runtime_settings(
            None,
            &model,
            Some(&config),
            true,
            reasoning_input(None, None),
        )
        .unwrap();

        assert_eq!(result.max_tokens, 32_000);
    }

    #[test]
    fn test_resolve_model_runtime_settings_prefers_model_reasoning_over_cli_default() {
        let mut model = model_entry(16_000);
        model.reasoning = Some(false);

        let result =
            resolve_model_runtime_settings(None, &model, None, true, reasoning_input(None, None))
                .unwrap();

        assert!(!result.reasoning);
    }

    #[test]
    fn test_resolve_model_runtime_settings_prefers_cli_reasoning_effort() {
        let mut model = model_entry(16_000);
        model.reasoning_effort = Some("low".to_string());

        let result = resolve_model_runtime_settings(
            None,
            &model,
            None,
            true,
            reasoning_input(Some("high"), Some("medium")),
        )
        .unwrap();

        assert_eq!(result.reasoning_effort, Some("high".to_string()));
    }

    #[test]
    fn test_resolve_model_runtime_settings_uses_model_reasoning_effort_before_env() {
        let mut model = model_entry(16_000);
        model.reasoning_effort = Some("low".to_string());

        let result = resolve_model_runtime_settings(
            None,
            &model,
            None,
            true,
            reasoning_input(None, Some("high")),
        )
        .unwrap();

        assert_eq!(result.reasoning_effort, Some("low".to_string()));
    }

    #[test]
    fn test_resolve_model_runtime_settings_uses_env_reasoning_effort_when_others_missing() {
        let model = model_entry(16_000);

        let result = resolve_model_runtime_settings(
            None,
            &model,
            None,
            true,
            reasoning_input(None, Some("medium")),
        )
        .unwrap();

        assert_eq!(result.reasoning_effort, Some("medium".to_string()));
    }

    #[test]
    fn test_resolve_model_runtime_settings_filters_empty_reasoning_effort() {
        let model = model_entry(16_000);

        let result = resolve_model_runtime_settings(
            None,
            &model,
            None,
            true,
            reasoning_input(Some(""), None),
        )
        .unwrap();

        assert_eq!(result.reasoning_effort, None);
    }

    #[test]
    fn test_resolve_model_runtime_settings_rejects_invalid_reasoning_effort() {
        let model = model_entry(16_000);

        let result = resolve_model_runtime_settings(
            None,
            &model,
            None,
            true,
            reasoning_input(Some("invalid"), None),
        );

        assert!(matches!(result, Err(error) if error.contains("reasoning_effort")));
    }
}
