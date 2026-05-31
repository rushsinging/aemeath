use super::*;

fn resolver_config() -> ModelsConfig {
    let mut providers = HashMap::new();
    providers.insert(
        "Zhipu".to_string(),
        ProviderModelsConfig {
            base_url: "https://zhipu.example.com".to_string(),
            api_key: "zhipu-key".to_string(),
            api: "zhipu".to_string(),
            models: vec![ModelEntryConfig {
                id: "glm-5.1".to_string(),
                name: "GLM 5.1".to_string(),
                context_window: 128_000,
                max_tokens: 32_000,
                reasoning: Some(true),
                ..Default::default()
            }],
        },
    );
    providers.insert(
        "LiteLLM".to_string(),
        ProviderModelsConfig {
            base_url: "https://litellm.example.com".to_string(),
            api_key: "litellm-key".to_string(),
            api: "litellm".to_string(),
            models: vec![ModelEntryConfig {
                id: "anthropic/claude-opus-4-7".to_string(),
                name: "Claude via LiteLLM".to_string(),
                context_window: 200_000,
                max_tokens: 16_000,
                reasoning: None,
                ..Default::default()
            }],
        },
    );
    ModelsConfig {
        mode: String::new(),
        default: "Zhipu/glm-5.1".to_string(),
        providers,
        guidance: HashMap::new(),
    }
}

#[test]
fn test_resolve_model_selection_zhipu() {
    let config = resolver_config();
    let resolved = config.resolve_model_selection("zhipu/glm-5.1").unwrap();
    assert_eq!(resolved.source_key, "Zhipu");
    assert_eq!(resolved.model.id, "glm-5.1");
    assert_eq!(resolved.api, "zhipu");
    assert_eq!(resolved.source_config.api, "zhipu");
}

#[test]
fn test_resolve_model_selection_litellm_model_id_with_slash() {
    let config = resolver_config();
    let resolved = config
        .resolve_model_selection("LiteLLM/anthropic/claude-opus-4-7")
        .unwrap();
    assert_eq!(resolved.source_key, "LiteLLM");
    assert_eq!(resolved.model.id, "anthropic/claude-opus-4-7");
    assert_eq!(resolved.api, "litellm");
}

#[test]
fn test_resolve_model_selection_unknown_source_lists_available() {
    let config = resolver_config();
    let err = config
        .resolve_model_selection("Missing/glm-5.1")
        .unwrap_err();
    let message = err.to_string();
    assert!(message.contains("未找到模型来源 'Missing'"));
    assert!(message.contains("Zhipu"));
    assert!(message.contains("LiteLLM"));
}

#[test]
fn test_resolve_model_selection_unknown_model_lists_available() {
    let config = resolver_config();
    let err = config.resolve_model_selection("Zhipu/glm-x").unwrap_err();
    let message = err.to_string();
    assert!(message.contains("来源 'Zhipu' 中未找到模型 'glm-x'"));
    assert!(message.contains("glm-5.1"));
}

#[test]
fn test_resolve_model_selection_preserves_api_string() {
    let mut config = resolver_config();
    let source = config.providers.get_mut("Zhipu").unwrap();
    source.api = "openai-compatible".to_string();

    let resolved = config.resolve_model_selection("Zhipu/glm-5.1").unwrap();

    assert_eq!(resolved.api, "openai-compatible");
}

#[test]
fn test_resolve_default_model_uses_config_default() {
    let config = resolver_config();
    let resolved = config.resolve_default_model().unwrap();
    assert_eq!(resolved.source_key, "Zhipu");
    assert_eq!(resolved.model.id, "glm-5.1");
}

#[test]
fn test_normalize_model_key() {
    assert_eq!(normalize_model_key("DeepSeek-V4-Pro ⚡"), "deepseek-v4-pro");
    assert_eq!(normalize_model_key("GPT-5.5"), "gpt-5.5");
    assert_eq!(normalize_model_key("GLM 5.1"), "glm5.1");
    assert_eq!(normalize_model_key(""), "");
}

#[test]
fn test_display_label() {
    let model = ModelEntryConfig {
        id: "glm-5.1".to_string(),
        name: "GLM 5.1 ⚡".to_string(),
        ..Default::default()
    };
    assert_eq!(model.display_label(), "GLM 5.1 ⚡ (id: glm-5.1)");

    let model2 = ModelEntryConfig {
        id: "gpt-5.5".to_string(),
        name: "".to_string(),
        ..Default::default()
    };
    assert_eq!(model2.display_label(), "gpt-5.5");

    let model3 = ModelEntryConfig {
        id: "gpt-5.5".to_string(),
        name: "gpt-5.5".to_string(),
        ..Default::default()
    };
    assert_eq!(model3.display_label(), "gpt-5.5");
}

#[test]
fn test_provider_ci() {
    let config = resolver_config();
    assert!(config.provider_ci("zhipu").is_some());
    assert!(config.provider_ci("ZHIPU").is_some());
    assert!(config.provider_ci("Zhipu").is_some());
    assert!(config.provider_ci("unknown").is_none());
}

#[test]
fn test_select_for_run_prefers_cli_model() {
    let config = resolver_config();
    let resolved = config
        .select_for_run(Some("LiteLLM/anthropic/claude-opus-4-7"))
        .unwrap();
    assert_eq!(resolved.source_key, "LiteLLM");
    assert_eq!(resolved.model.id, "anthropic/claude-opus-4-7");
}

#[test]
fn test_select_for_run_falls_back_to_default_when_none() {
    let config = resolver_config();
    let resolved = config.select_for_run(None).unwrap();
    assert_eq!(resolved.source_key, "Zhipu");
    assert_eq!(resolved.model.id, "glm-5.1");
}

#[test]
fn test_select_for_run_treats_empty_as_none() {
    let config = resolver_config();
    let resolved = config.select_for_run(Some("")).unwrap();
    assert_eq!(resolved.source_key, "Zhipu");
    assert_eq!(resolved.model.id, "glm-5.1");
}
