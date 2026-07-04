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
            reasoning: None,
        },
        driver: driver.as_str().to_string(),
    }
}

fn runtime_settings(reasoning: bool) -> ModelRuntimeSettings {
    ModelRuntimeSettings {
        max_tokens: 16_000,
        reasoning,
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
fn test_openai_config_skips_ollama() {
    let result = openai_config(ProviderDriverKind::Ollama, "ollama");

    assert!(result.is_none());
}

#[test]
fn test_build_llm_client_ollama_constructs_ollama_provider() {
    let resolved = resolved_model(ProviderDriverKind::Ollama, "", "", "ollama");
    let settings = runtime_settings(false);

    let client = build_llm_client(
        ProviderDriverKind::Ollama,
        "ollama".to_string(),
        Some("http://localhost:11434".to_string()),
        "llama3.2".to_string(),
        &resolved,
        &settings,
        None,
    );

    assert_eq!(client.provider_name(), "ollama");
}

#[test]
fn test_provider_driver_api_key_env_name_ollama() {
    assert_eq!(
        share::config::domain::driver_env::driver_api_key_env_name("ollama"),
        Some("OLLAMA_API_KEY")
    );
}

#[test]
fn test_provider_driver_api_key_env_name_minimax() {
    assert_eq!(
        share::config::domain::driver_env::driver_api_key_env_name("minimax"),
        Some("MINIMAX_API_KEY")
    );
}

#[test]
fn test_provider_driver_api_key_env_name_mimo() {
    assert_eq!(
        share::config::domain::driver_env::driver_api_key_env_name("mimo"),
        Some("MIMO_API_KEY")
    );
}

#[test]
fn test_build_llm_client_sets_reasoning_level() {
    let resolved = resolved_model(ProviderDriverKind::OpenAI, "", "", "OpenAI");

    let settings = runtime_settings(true);
    let client = build_llm_client(
        ProviderDriverKind::OpenAI,
        "key".to_string(),
        None,
        "model-id".to_string(),
        &resolved,
        &settings,
        None,
    );

    assert_eq!(
        client.current_reasoning_level(),
        provider::contract::ReasoningLevel::Medium
    );
}

#[test]
fn test_build_llm_client_reasoning_false_sets_off() {
    let resolved = resolved_model(ProviderDriverKind::OpenAI, "", "", "OpenAI");

    let settings = runtime_settings(false);
    let client = build_llm_client(
        ProviderDriverKind::OpenAI,
        "key".to_string(),
        None,
        "model-id".to_string(),
        &resolved,
        &settings,
        None,
    );

    assert_eq!(
        client.current_reasoning_level(),
        provider::contract::ReasoningLevel::Off
    );
}
