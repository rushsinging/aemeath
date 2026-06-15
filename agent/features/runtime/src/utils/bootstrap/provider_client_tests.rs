use super::*;
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
            thinking_max_tokens: 0,
            reasoning: None,
            reasoning_effort: None,
        },
        driver: driver.as_str().to_string(),
    }
}

fn runtime_settings(
    thinking_max_tokens: u32,
    reasoning: bool,
    reasoning_effort: Option<&str>,
) -> ModelRuntimeSettings {
    ModelRuntimeSettings {
        max_tokens: 16_000,
        thinking_max_tokens,
        reasoning,
        reasoning_effort: reasoning_effort.map(str::to_string),
    }
}

fn env_reader<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
    move |name| {
        pairs
            .iter()
            .find(|(key, _)| *key == name)
            .map(|(_, value)| (*value).to_string())
    }
}

#[test]
fn test_resolve_api_key_prefers_cli_key() {
    let resolved = resolved_model(ProviderDriverKind::OpenAI, "config-key", "", "openai");
    let read_env = env_reader(&[("AEMEATH_API_KEY", "env-key")]);

    let result = resolve_api_key(Some("cli-key".to_string()), &resolved, Some(&read_env));

    assert_eq!(result, Some("cli-key".to_string()));
}

#[test]
fn test_resolve_api_key_uses_aemeath_env_before_provider_env() {
    let resolved = resolved_model(ProviderDriverKind::OpenAI, "config-key", "", "openai");
    let read_env = env_reader(&[
        ("AEMEATH_API_KEY", "aemeath-key"),
        ("OPENAI_API_KEY", "openai-key"),
    ]);

    let result = resolve_api_key(None, &resolved, Some(&read_env));

    assert_eq!(result, Some("aemeath-key".to_string()));
}

#[test]
fn test_resolve_api_key_uses_provider_env_before_llm_env() {
    let resolved = resolved_model(ProviderDriverKind::Anthropic, "config-key", "", "anthropic");
    let read_env = env_reader(&[
        ("ANTHROPIC_API_KEY", "anthropic-key"),
        ("LLM_API_KEY", "llm-key"),
    ]);

    let result = resolve_api_key(None, &resolved, Some(&read_env));

    assert_eq!(result, Some("anthropic-key".to_string()));
}

#[test]
fn test_resolve_api_key_uses_llm_env_for_litellm_without_provider_env() {
    let resolved = resolved_model(ProviderDriverKind::LiteLLM, "config-key", "", "litellm");
    let read_env = env_reader(&[("LLM_API_KEY", "llm-key")]);

    let result = resolve_api_key(None, &resolved, Some(&read_env));

    assert_eq!(result, Some("llm-key".to_string()));
}

#[test]
fn test_resolve_api_key_uses_minimax_provider_env_before_llm_env() {
    let resolved = resolved_model(ProviderDriverKind::Minimax, "config-key", "", "minimax");
    let read_env = env_reader(&[
        ("MINIMAX_API_KEY", "minimax-key"),
        ("LLM_API_KEY", "llm-key"),
    ]);

    let result = resolve_api_key(None, &resolved, Some(&read_env));

    assert_eq!(result, Some("minimax-key".to_string()));
}

#[test]
fn test_resolve_api_key_uses_mimo_provider_env_before_llm_env() {
    let resolved = resolved_model(ProviderDriverKind::Mimo, "config-key", "", "mimo");
    let read_env = env_reader(&[("MIMO_API_KEY", "mimo-key"), ("LLM_API_KEY", "llm-key")]);

    let result = resolve_api_key(None, &resolved, Some(&read_env));

    assert_eq!(result, Some("mimo-key".to_string()));
}

#[test]
fn test_resolve_api_key_uses_config_key_when_env_missing() {
    let resolved = resolved_model(ProviderDriverKind::Zhipu, "config-key", "", "zhipu");
    let read_env = env_reader(&[]);

    let result = resolve_api_key(None, &resolved, Some(&read_env));

    assert_eq!(result, Some("config-key".to_string()));
}

#[test]
fn test_resolve_api_key_returns_none_when_all_sources_missing() {
    let resolved = resolved_model(ProviderDriverKind::Zhipu, "", "", "zhipu");
    let read_env = env_reader(&[]);

    let result = resolve_api_key(None, &resolved, Some(&read_env));

    assert_eq!(result, None);
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
fn test_openai_config_skips_anthropic() {
    let result = openai_config(ProviderDriverKind::Anthropic, "anthropic");

    assert!(result.is_none());
}

#[test]
fn test_openai_config_uses_source_key_for_openai_compatible() {
    let result = openai_config(ProviderDriverKind::Zhipu, "Zhipu").unwrap();

    assert_eq!(result.source_key, "Zhipu");
    assert_eq!(result.driver, ProviderDriverKind::Zhipu);
}

#[test]
fn test_reasoning_config_prefers_reasoning_effort() {
    let settings = runtime_settings(4096, true, Some("high"));

    let result = reasoning_config(&settings, Some(false));

    assert!(matches!(result, Some(ReasoningConfig::Object(_))));
}

#[test]
fn test_reasoning_config_uses_thinking_budget_before_model_reasoning() {
    let settings = runtime_settings(4096, true, None);

    let result = reasoning_config(&settings, Some(true));

    assert!(matches!(
        result,
        Some(ReasoningConfig::ThinkingBudget(4096))
    ));
}

#[test]
fn test_reasoning_config_exact_filter_path_documents_stop_hook_command() {
    let exact_path = "utils::bootstrap::provider_client::tests::test_reasoning_config_uses_thinking_budget_before_model_reasoning";

    assert!(
        exact_path.ends_with("::test_reasoning_config_uses_thinking_budget_before_model_reasoning")
    );
    assert_ne!(
        exact_path,
        "test_reasoning_config_uses_thinking_budget_before_model_reasoning"
    );
}

#[test]
fn test_reasoning_config_thinking_budget_respects_model_reasoning_false() {
    // 当 model_reasoning == Some(false) 时，即使 thinking_max_tokens > 0
    // 也不应强制开启 thinking，应返回 Bool(false)。
    let settings = runtime_settings(8192, false, None);

    let result = reasoning_config(&settings, Some(false));

    assert!(matches!(result, Some(ReasoningConfig::Bool(false))));
}

#[test]
fn test_reasoning_config_uses_model_reasoning_without_budget_or_effort() {
    let settings = runtime_settings(0, true, None);

    let result = reasoning_config(&settings, Some(false));

    assert!(matches!(result, Some(ReasoningConfig::Bool(false))));
}

#[test]
fn test_openai_config_skips_ollama() {
    // 回归 #85：Ollama 有专用 OllamaProvider，不应生成 openai_config，
    // 否则 from_config 会把它错误地路由到 OpenAI 兼容工厂分支。
    let result = openai_config(ProviderDriverKind::Ollama, "ollama");

    assert!(result.is_none());
}

#[test]
fn test_build_llm_client_ollama_constructs_ollama_provider() {
    // 回归 #85：config 中 driver="ollama" 必须由工厂构造出 OllamaProvider，
    // 修复前会回退到 ProviderDriverKind::OpenAI 并构造 OpenAICompatibleProvider。
    let resolved = resolved_model(ProviderDriverKind::Ollama, "", "", "ollama");
    let settings = runtime_settings(0, false, None);

    let client = build_llm_client(
        ProviderDriverKind::Ollama,
        "ollama".to_string(),
        Some("http://localhost:11434".to_string()),
        "llama3.2".to_string(),
        &resolved,
        &settings,
    );

    assert_eq!(client.provider_name(), "ollama");
}

#[test]
fn test_provider_driver_api_key_env_name_ollama() {
    assert_eq!(
        provider_driver_api_key_env_name(ProviderDriverKind::Ollama),
        Some("OLLAMA_API_KEY")
    );
}

#[test]
fn test_provider_driver_api_key_env_name_minimax() {
    assert_eq!(
        provider_driver_api_key_env_name(ProviderDriverKind::Minimax),
        Some("MINIMAX_API_KEY")
    );
}

#[test]
fn test_provider_driver_api_key_env_name_mimo() {
    assert_eq!(
        provider_driver_api_key_env_name(ProviderDriverKind::Mimo),
        Some("MIMO_API_KEY")
    );
}

#[test]
fn test_build_llm_client_sets_reasoning_effort() {
    let resolved = resolved_model(ProviderDriverKind::OpenAI, "", "", "OpenAI");

    let settings = runtime_settings(0, true, Some("high"));
    let client = build_llm_client(
        ProviderDriverKind::OpenAI,
        "key".to_string(),
        None,
        "model-id".to_string(),
        &resolved,
        &settings,
    );

    assert_eq!(client.reasoning_effort(), Some("high".to_string()));
}
