use share::config::models::ModelEntryConfig;

pub struct ModelRuntimeSettings {
    pub max_tokens: u32,
    pub reasoning: bool,
    /// 模型配置的固定推理档位（"off".."max"）。None 时沿用 reasoning bool 映射。
    pub reasoning_effort: Option<String>,
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
        reasoning_effort: model.reasoning_effort.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model_entry(reasoning: Option<bool>) -> ModelEntryConfig {
        ModelEntryConfig {
            id: "model-id".to_string(),
            name: "model-name".to_string(),
            input: Vec::new(),
            context_window: 128_000,
            max_tokens: 16_000,
            reasoning,
            reasoning_effort: None,
            api_style: None,
        }
    }

    #[test]
    fn resolve_model_runtime_settings_uses_resolved_max_tokens() {
        assert_eq!(
            resolve_model_runtime_settings(8_192, &model_entry(None), true).max_tokens,
            8_192
        );
    }

    #[test]
    fn resolve_model_runtime_settings_prefers_model_reasoning_over_cli_default() {
        assert!(!resolve_model_runtime_settings(8_192, &model_entry(Some(false)), true).reasoning);
    }

    #[test]
    fn resolve_model_runtime_settings_uses_cli_reasoning_default_when_model_missing() {
        assert!(resolve_model_runtime_settings(8_192, &model_entry(None), true).reasoning);
    }

    #[test]
    fn resolve_model_runtime_settings_passes_through_reasoning_effort() {
        let mut model = model_entry(Some(true));
        model.reasoning_effort = Some("xhigh".to_string());
        assert_eq!(
            resolve_model_runtime_settings(8_192, &model, true)
                .reasoning_effort
                .as_deref(),
            Some("xhigh")
        );
    }

    #[test]
    fn resolve_model_runtime_settings_reasoning_effort_none_by_default() {
        assert_eq!(
            resolve_model_runtime_settings(8_192, &model_entry(Some(true)), true).reasoning_effort,
            None
        );
    }
}
