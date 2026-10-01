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
