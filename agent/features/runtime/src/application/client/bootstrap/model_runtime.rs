use share::config::models::ModelEntryConfig;

pub struct ModelRuntimeSettingsData {
    pub max_tokens: u32,
    pub reasoning: bool,
    /// 模型配置的固定推理档位（"off".."max"）。None 时沿用 reasoning bool 映射。
    pub reasoning_effort: Option<String>,
}

pub fn resolve_model_runtime_settings(
    resolved_max_tokens: u32,
    model: &ModelEntryConfig,
    cli_reasoning_default: bool,
) -> ModelRuntimeSettingsData {
    let reasoning = model.reasoning.unwrap_or(cli_reasoning_default);

    ModelRuntimeSettingsData {
        max_tokens: resolved_max_tokens,
        reasoning,
        reasoning_effort: model.reasoning_effort.clone(),
    }
}

#[cfg(test)]
#[path = "model_runtime_tests.rs"]
mod tests;
