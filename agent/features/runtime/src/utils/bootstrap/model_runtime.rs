use share::config::models::ModelEntryConfig;

pub struct ModelRuntimeSettings {
    pub max_tokens: u32,
    pub reasoning: bool,
}

pub fn resolve_model_runtime_settings(
    resolved_max_tokens: u32,
    model: &ModelEntryConfig,
    cli_reasoning_default: bool,
) -> ModelRuntimeSettings {
    let reasoning = model.reasoning.unwrap_or(cli_reasoning_default);

    ModelRuntimeSettings {
        max_tokens: resolved_max_tokens,
        reasoning,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use share::config::models::ModelEntryConfig;

    fn model_entry(reasoning: Option<bool>) -> ModelEntryConfig {
        ModelEntryConfig {
            id: "model-id".to_string(),
            name: "model-name".to_string(),
            input: Vec::new(),
            context_window: 128_000,
            max_tokens: 16_000,
            reasoning,
        }
    }

    #[test]
    fn test_resolve_model_runtime_settings_uses_resolved_max_tokens() {
        let model = model_entry(None);

        let result = resolve_model_runtime_settings(8_192, &model, true);

        assert_eq!(result.max_tokens, 8_192);
    }

    #[test]
    fn test_resolve_model_runtime_settings_prefers_model_reasoning_over_cli_default() {
        let model = model_entry(Some(false));

        let result = resolve_model_runtime_settings(8_192, &model, true);

        assert!(!result.reasoning);
    }

    #[test]
    fn test_resolve_model_runtime_settings_uses_cli_reasoning_default_when_model_missing() {
        let model = model_entry(None);

        let result = resolve_model_runtime_settings(8_192, &model, true);

        assert!(result.reasoning);
    }
}
