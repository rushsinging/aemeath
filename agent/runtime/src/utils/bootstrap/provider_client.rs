use super::model_runtime::ModelRuntimeSettings;
use crate::api::core::config::models::ResolvedModel;
use crate::api::provider::client::{LlmClient, LlmConfigOptions, OpenAIProviderConfig};
use crate::api::provider::providers::openai_compatible::ReasoningConfig;
use crate::api::provider::ApiDriverKind;
use std::env;

type EnvReader<'a> = Option<&'a dyn Fn(&str) -> Option<String>>;

pub fn resolve_api_key(
    cli_api_key: Option<String>,
    resolved_model: &ResolvedModel,
    env_value: EnvReader<'_>,
) -> Option<String> {
    let api_type = ApiDriverKind::parse(&resolved_model.api).unwrap_or(ApiDriverKind::OpenAI);
    cli_api_key
        .or_else(|| env_or_runtime("AEMEATH_API_KEY", env_value))
        .or_else(|| provider_api_key_from_env(api_type, env_value))
        .or_else(|| env_or_runtime("LLM_API_KEY", env_value))
        .or_else(|| non_empty_string(&resolved_model.source_config.api_key))
}

pub fn resolve_base_url(
    cli_base_url: Option<String>,
    resolved_model: &ResolvedModel,
) -> Option<String> {
    cli_base_url.or_else(|| non_empty_string(&resolved_model.source_config.base_url))
}

pub fn build_llm_client(
    api_type: ApiDriverKind,
    api_key: String,
    base_url: Option<String>,
    model: String,
    resolved_model: &ResolvedModel,
    runtime_settings: &ModelRuntimeSettings,
) -> LlmClient {
    let reasoning_effort = runtime_settings.reasoning_effort.clone();
    let client = LlmClient::from_config(LlmConfigOptions {
        api: api_type,
        api_key,
        base_url,
        model,
        max_tokens: runtime_settings.max_tokens,
        thinking_max_tokens: runtime_settings.thinking_max_tokens,
        reasoning: runtime_settings.reasoning,
        reasoning_config: reasoning_config(runtime_settings, resolved_model.model.reasoning),
        openai_config: openai_config(api_type, &resolved_model.source_key),
    });

    if let Some(effort) = reasoning_effort {
        client.set_reasoning_effort(Some(effort));
    }

    client
}

fn provider_api_key_from_env(api_type: ApiDriverKind, env_value: EnvReader<'_>) -> Option<String> {
    provider_api_key_env_name(api_type).and_then(|name| env_or_runtime(name, env_value))
}

fn provider_api_key_env_name(api_type: ApiDriverKind) -> Option<&'static str> {
    match api_type {
        ApiDriverKind::Anthropic => Some("ANTHROPIC_API_KEY"),
        ApiDriverKind::OpenAI => Some("OPENAI_API_KEY"),
        ApiDriverKind::Volcengine => Some("VOLCENGINE_CODING_PLAN_API_KEY"),
        ApiDriverKind::Zhipu | ApiDriverKind::LiteLLM => None,
    }
}

fn env_or_runtime(name: &str, env_value: EnvReader<'_>) -> Option<String> {
    if let Some(read_env) = env_value {
        return read_env(name);
    }

    env::var(name).ok()
}

fn non_empty_string(value: &str) -> Option<String> {
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn openai_config(api_type: ApiDriverKind, source_key: &str) -> Option<OpenAIProviderConfig> {
    if matches!(api_type, ApiDriverKind::Anthropic) {
        None
    } else {
        Some(OpenAIProviderConfig::from_api_driver(api_type, source_key))
    }
}

fn reasoning_config(
    runtime_settings: &ModelRuntimeSettings,
    model_reasoning: Option<bool>,
) -> Option<ReasoningConfig> {
    runtime_settings
        .reasoning_effort
        .as_ref()
        .map(|effort| ReasoningConfig::Object(serde_json::json!({ "effort": effort })))
        .or_else(|| {
            if runtime_settings.thinking_max_tokens > 0 {
                Some(ReasoningConfig::ThinkingBudget(
                    runtime_settings.thinking_max_tokens,
                ))
            } else {
                model_reasoning.map(ReasoningConfig::Bool)
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::core::config::models::{ModelEntryConfig, ProviderModelsConfig};

    fn resolved_model(
        api: ApiDriverKind,
        api_key: &str,
        base_url: &str,
        source_key: &str,
    ) -> ResolvedModel {
        ResolvedModel {
            source_key: source_key.to_string(),
            source_config: ProviderModelsConfig {
                api_key: api_key.to_string(),
                base_url: base_url.to_string(),
                api: api.as_str().to_string(),
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
            api: api.as_str().to_string(),
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
        let resolved = resolved_model(ApiDriverKind::OpenAI, "config-key", "", "openai");
        let read_env = env_reader(&[("AEMEATH_API_KEY", "env-key")]);

        let result = resolve_api_key(Some("cli-key".to_string()), &resolved, Some(&read_env));

        assert_eq!(result, Some("cli-key".to_string()));
    }

    #[test]
    fn test_resolve_api_key_uses_aemeath_env_before_provider_env() {
        let resolved = resolved_model(ApiDriverKind::OpenAI, "config-key", "", "openai");
        let read_env = env_reader(&[
            ("AEMEATH_API_KEY", "aemeath-key"),
            ("OPENAI_API_KEY", "openai-key"),
        ]);

        let result = resolve_api_key(None, &resolved, Some(&read_env));

        assert_eq!(result, Some("aemeath-key".to_string()));
    }

    #[test]
    fn test_resolve_api_key_uses_provider_env_before_llm_env() {
        let resolved = resolved_model(ApiDriverKind::Anthropic, "config-key", "", "anthropic");
        let read_env = env_reader(&[
            ("ANTHROPIC_API_KEY", "anthropic-key"),
            ("LLM_API_KEY", "llm-key"),
        ]);

        let result = resolve_api_key(None, &resolved, Some(&read_env));

        assert_eq!(result, Some("anthropic-key".to_string()));
    }

    #[test]
    fn test_resolve_api_key_uses_llm_env_for_litellm_without_provider_env() {
        let resolved = resolved_model(ApiDriverKind::LiteLLM, "config-key", "", "litellm");
        let read_env = env_reader(&[("LLM_API_KEY", "llm-key")]);

        let result = resolve_api_key(None, &resolved, Some(&read_env));

        assert_eq!(result, Some("llm-key".to_string()));
    }

    #[test]
    fn test_resolve_api_key_uses_config_key_when_env_missing() {
        let resolved = resolved_model(ApiDriverKind::Zhipu, "config-key", "", "zhipu");
        let read_env = env_reader(&[]);

        let result = resolve_api_key(None, &resolved, Some(&read_env));

        assert_eq!(result, Some("config-key".to_string()));
    }

    #[test]
    fn test_resolve_api_key_returns_none_when_all_sources_missing() {
        let resolved = resolved_model(ApiDriverKind::Zhipu, "", "", "zhipu");
        let read_env = env_reader(&[]);

        let result = resolve_api_key(None, &resolved, Some(&read_env));

        assert_eq!(result, None);
    }

    #[test]
    fn test_resolve_base_url_prefers_cli_base_url() {
        let resolved = resolved_model(
            ApiDriverKind::OpenAI,
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
            ApiDriverKind::OpenAI,
            "",
            "https://config.example",
            "openai",
        );

        let result = resolve_base_url(None, &resolved);

        assert_eq!(result, Some("https://config.example".to_string()));
    }

    #[test]
    fn test_resolve_base_url_returns_none_when_missing() {
        let resolved = resolved_model(ApiDriverKind::OpenAI, "", "", "openai");

        let result = resolve_base_url(None, &resolved);

        assert_eq!(result, None);
    }

    #[test]
    fn test_openai_config_skips_anthropic() {
        let result = openai_config(ApiDriverKind::Anthropic, "anthropic");

        assert!(result.is_none());
    }

    #[test]
    fn test_openai_config_uses_source_key_for_openai_compatible() {
        let result = openai_config(ApiDriverKind::Zhipu, "Zhipu").unwrap();

        assert_eq!(result.source_key, "Zhipu");
        assert_eq!(result.api, ApiDriverKind::Zhipu);
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

        let result = reasoning_config(&settings, Some(false));

        assert!(matches!(
            result,
            Some(ReasoningConfig::ThinkingBudget(4096))
        ));
    }

    #[test]
    fn test_reasoning_config_uses_model_reasoning_without_budget_or_effort() {
        let settings = runtime_settings(0, true, None);

        let result = reasoning_config(&settings, Some(false));

        assert!(matches!(result, Some(ReasoningConfig::Bool(false))));
    }

    #[test]
    fn test_build_llm_client_sets_reasoning_effort() {
        let resolved = resolved_model(ApiDriverKind::OpenAI, "", "", "OpenAI");

        let settings = runtime_settings(0, true, Some("high"));
        let client = build_llm_client(
            ApiDriverKind::OpenAI,
            "key".to_string(),
            None,
            "model-id".to_string(),
            &resolved,
            &settings,
        );

        assert_eq!(client.reasoning_effort(), Some("high".to_string()));
    }
}
