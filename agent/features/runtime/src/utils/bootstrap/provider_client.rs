use super::model_runtime::ModelRuntimeSettings;
use provider::api::ProviderDriverKind;
use provider::api::{LlmClient, LlmConfigOptions, OpenAIProviderConfig};
use provider::contract::ReasoningLevel;
use share::config::models::ResolvedModel;
use std::env;

type EnvReader<'a> = Option<&'a dyn Fn(&str) -> Option<String>>;

pub fn resolve_api_key(
    cli_api_key: Option<String>,
    resolved_model: &ResolvedModel,
    env_value: EnvReader<'_>,
) -> Option<String> {
    let driver =
        ProviderDriverKind::parse(&resolved_model.driver).unwrap_or(ProviderDriverKind::OpenAI);
    cli_api_key
        .or_else(|| env_or_runtime("AEMEATH_API_KEY", env_value))
        .or_else(|| provider_driver_api_key_from_env(driver, env_value))
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
    driver: ProviderDriverKind,
    api_key: String,
    base_url: Option<String>,
    model: String,
    resolved_model: &ResolvedModel,
    runtime_settings: &ModelRuntimeSettings,
) -> LlmClient {
    let client = LlmClient::from_config(LlmConfigOptions {
        driver,
        api_key,
        base_url,
        model,
        max_tokens: runtime_settings.max_tokens,
        reasoning: runtime_settings.reasoning,
        reasoning_config: None,
        openai_config: openai_config(driver, &resolved_model.source_key),
    });

    // reasoning: bool → ReasoningLevel 映射：
    // true  → Medium（用户未指定档位时的合理默认）
    // false → Off
    let level = if runtime_settings.reasoning {
        ReasoningLevel::Medium
    } else {
        ReasoningLevel::Off
    };
    client.set_reasoning_level(level);

    client
}

fn provider_driver_api_key_from_env(
    driver: ProviderDriverKind,
    env_value: EnvReader<'_>,
) -> Option<String> {
    provider_driver_api_key_env_name(driver).and_then(|name| env_or_runtime(name, env_value))
}

fn provider_driver_api_key_env_name(driver: ProviderDriverKind) -> Option<&'static str> {
    match driver {
        ProviderDriverKind::Anthropic => Some("ANTHROPIC_API_KEY"),
        ProviderDriverKind::OpenAI => Some("OPENAI_API_KEY"),
        ProviderDriverKind::Volcengine => Some("VOLCENGINE_CODING_PLAN_API_KEY"),
        ProviderDriverKind::Minimax => Some("MINIMAX_API_KEY"),
        ProviderDriverKind::Mimo => Some("MIMO_API_KEY"),
        ProviderDriverKind::Ollama => Some("OLLAMA_API_KEY"),
        ProviderDriverKind::Zhipu | ProviderDriverKind::LiteLLM => None,
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

fn openai_config(driver: ProviderDriverKind, source_key: &str) -> Option<OpenAIProviderConfig> {
    // Anthropic 与 Ollama 各有专用 provider，不走 OpenAI 兼容工厂分支。
    if matches!(
        driver,
        ProviderDriverKind::Anthropic | ProviderDriverKind::Ollama
    ) {
        None
    } else {
        Some(OpenAIProviderConfig::from_driver(driver, source_key))
    }
}

#[cfg(test)]
#[path = "provider_client_tests.rs"]
mod tests;
