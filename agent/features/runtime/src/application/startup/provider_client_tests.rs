use super::*;
use provider::ProviderDriverKind;
use share::config::models::{ModelEntryConfig, ProviderModelsConfig};

fn resolved_model(
    driver: ProviderDriverKind,
    api_key: &str,
    base_url: &str,
    source_key: &str,
) -> ResolvedModel {
    ResolvedModel {
        source_key: source_key.to_string(),
        source_config: ProviderModelsConfig {
            api_key: api_key.to_string(),
            base_url: base_url.to_string(),
            driver: driver.as_str().to_string(),
            models: Vec::new(),
        },
        model: ModelEntryConfig {
            id: "model-id".to_string(),
            name: "model-name".to_string(),
            input: Vec::new(),
            context_window: 128_000,
            max_tokens: 16_000,
            reasoning: None,
            reasoning_effort: None,
            api_style: None,
        },
        driver: driver.as_str().to_string(),
    }
}

fn runtime_settings(reasoning: bool) -> ModelRuntimeSettings {
    ModelRuntimeSettings {
        max_tokens: 16_000,
        reasoning,
        reasoning_effort: None,
    }
}

fn runtime_settings_with_effort(reasoning: bool, effort: &str) -> ModelRuntimeSettings {
    ModelRuntimeSettings {
        max_tokens: 16_000,
        reasoning,
        reasoning_effort: Some(effort.to_string()),
    }
}

#[test]
fn test_resolve_api_key_uses_resolved_config_only() {
    let resolved = resolved_model(ProviderDriverKind::OpenAI, "config-key", "", "openai");
    assert_eq!(resolve_api_key(&resolved), Some("config-key".to_string()));
}

#[test]
fn test_resolve_api_key_returns_none_when_config_missing() {
    let resolved = resolved_model(ProviderDriverKind::Zhipu, "", "", "zhipu");
    assert_eq!(resolve_api_key(&resolved), None);
}

#[test]
fn test_resolve_base_url_prefers_cli_base_url() {
    let resolved = resolved_model(
        ProviderDriverKind::OpenAI,
        "",
        "https://config.example",
        "openai",
    );

    let result = resolve_base_url(Some("https://cli.example".to_string()), &resolved);

    assert_eq!(result, Some("https://cli.example".to_string()));
}

#[test]
fn test_resolve_base_url_uses_config_base_url() {
    let resolved = resolved_model(
        ProviderDriverKind::OpenAI,
        "",
        "https://config.example",
        "openai",
    );

    let result = resolve_base_url(None, &resolved);

    assert_eq!(result, Some("https://config.example".to_string()));
}

#[test]
fn test_resolve_base_url_returns_none_when_missing() {
    let resolved = resolved_model(ProviderDriverKind::OpenAI, "", "", "openai");

    let result = resolve_base_url(None, &resolved);

    assert_eq!(result, None);
}

#[test]
fn test_build_llm_client_ollama_constructs_ollama_provider() {
    let resolved = resolved_model(ProviderDriverKind::Ollama, "", "", "ollama");
    let settings = runtime_settings(false);

    let client = build_llm_client(
        ProviderDriverKind::Ollama.as_str(),
        "ollama".to_string(),
        Some("http://localhost:11434".to_string()),
        "llama3.2".to_string(),
        &resolved,
        &settings,
        None,
        30,
    )
    .expect("valid provider client config");

    assert_eq!(client.provider_name(), "ollama");
}

#[test]
fn test_build_llm_client_sets_reasoning_level() {
    let resolved = resolved_model(ProviderDriverKind::OpenAI, "", "", "OpenAI");

    let settings = runtime_settings(true);
    let client = build_llm_client(
        ProviderDriverKind::OpenAI.as_str(),
        "key".to_string(),
        None,
        "model-id".to_string(),
        &resolved,
        &settings,
        None,
        30,
    )
    .expect("valid provider client config");

    assert_eq!(
        client.default_scope().effective_reasoning(),
        provider::ReasoningLevel::Medium
    );
}

#[test]
fn test_build_llm_client_reasoning_false_sets_off() {
    let resolved = resolved_model(ProviderDriverKind::OpenAI, "", "", "OpenAI");

    let settings = runtime_settings(false);
    let client = build_llm_client(
        ProviderDriverKind::OpenAI.as_str(),
        "key".to_string(),
        None,
        "model-id".to_string(),
        &resolved,
        &settings,
        None,
        30,
    )
    .expect("valid provider client config");

    assert_eq!(
        client.default_scope().effective_reasoning(),
        provider::ReasoningLevel::Off
    );
}

#[test]
fn test_build_llm_client_reasoning_effort_overrides_bool_default() {
    // Zhipu 上限为 Max，xhigh 不会被 clamp，验证 effort 覆盖了 bool→Medium 默认。
    let resolved = resolved_model(ProviderDriverKind::Zhipu, "", "", "Zhipu");

    let settings = runtime_settings_with_effort(true, "xhigh");
    let client = build_llm_client(
        ProviderDriverKind::Zhipu.as_str(),
        "key".to_string(),
        None,
        "model-id".to_string(),
        &resolved,
        &settings,
        None,
        30,
    )
    .expect("valid provider client config");

    assert_eq!(
        client.default_scope().effective_reasoning(),
        provider::ReasoningLevel::Xhigh
    );
}

#[test]
fn test_build_llm_client_reasoning_effort_clamped_to_provider_ceiling() {
    // OpenAI 上限为 High，配置 max 会被 clamp 到 High。
    let resolved = resolved_model(ProviderDriverKind::OpenAI, "", "", "OpenAI");

    let settings = runtime_settings_with_effort(true, "max");
    let client = build_llm_client(
        ProviderDriverKind::OpenAI.as_str(),
        "key".to_string(),
        None,
        "model-id".to_string(),
        &resolved,
        &settings,
        None,
        30,
    )
    .expect("valid provider client config");

    assert_eq!(
        client.default_scope().effective_reasoning(),
        provider::ReasoningLevel::High
    );
}

#[test]
fn test_build_llm_client_reasoning_effort_off_disables_thinking() {
    // reasoning=true 但 effort="off" 时，显式档位优先，最终为 Off。
    let resolved = resolved_model(ProviderDriverKind::Zhipu, "", "", "Zhipu");

    let settings = runtime_settings_with_effort(true, "off");
    let client = build_llm_client(
        ProviderDriverKind::Zhipu.as_str(),
        "key".to_string(),
        None,
        "model-id".to_string(),
        &resolved,
        &settings,
        None,
        30,
    )
    .expect("valid provider client config");

    assert_eq!(
        client.default_scope().effective_reasoning(),
        provider::ReasoningLevel::Off
    );
}
