use share::config::models::ModelEntryConfig;
use share::config::Config;

pub struct ModelRuntimeSettings {
    pub max_tokens: u32,
    pub reasoning: bool,
}

pub fn resolve_model_runtime_settings(
    cli_max_tokens: Option<u32>,
    model: &ModelEntryConfig,
    config_file: Option<&Config>,
    cli_reasoning_default: bool,
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
    let reasoning = model.reasoning.unwrap_or(cli_reasoning_default);

    Ok(ModelRuntimeSettings {
        max_tokens,
        reasoning,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use share::config::models::ModelEntryConfig;
    use share::config::{Config, ModelConfig};

    fn model_entry(max_tokens: u32) -> ModelEntryConfig {
        ModelEntryConfig {
            id: "model-id".to_string(),
            name: "model-name".to_string(),
            input: Vec::new(),
            context_window: 128_000,
            max_tokens,
            reasoning: None,
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

    #[test]
    fn test_resolve_model_runtime_settings_prefers_cli_max_tokens() {
        let model = model_entry(16_000);
        let config = config_with_max_tokens(24_000);

        let result =
            resolve_model_runtime_settings(Some(8_000), &model, Some(&config), true).unwrap();

        assert_eq!(result.max_tokens, 8_000);
    }

    #[test]
    fn test_resolve_model_runtime_settings_uses_model_max_tokens() {
        let model = model_entry(16_000);
        let config = config_with_max_tokens(24_000);

        let result = resolve_model_runtime_settings(None, &model, Some(&config), true).unwrap();

        assert_eq!(result.max_tokens, 16_000);
    }

    #[test]
    fn test_resolve_model_runtime_settings_uses_config_max_tokens_when_model_zero() {
        let model = model_entry(0);
        let config = config_with_max_tokens(24_000);

        let result = resolve_model_runtime_settings(None, &model, Some(&config), true).unwrap();

        assert_eq!(result.max_tokens, 24_000);
    }

    #[test]
    fn test_resolve_model_runtime_settings_uses_default_max_tokens_when_missing() {
        let model = model_entry(0);
        let config = config_with_max_tokens(0);

        let result = resolve_model_runtime_settings(None, &model, Some(&config), true).unwrap();

        assert_eq!(result.max_tokens, 32_000);
    }

    #[test]
    fn test_resolve_model_runtime_settings_prefers_model_reasoning_over_cli_default() {
        let mut model = model_entry(16_000);
        model.reasoning = Some(false);

        let result = resolve_model_runtime_settings(None, &model, None, true).unwrap();

        assert!(!result.reasoning);
    }
}
