use super::model_runtime::ModelRuntimeSettings;
use provider::api::openai_compatible::ReasoningConfig;
use provider::api::ProviderDriverKind;
use provider::api::{LlmClient, LlmConfigOptions, OpenAIProviderConfig};
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
    let reasoning_effort = runtime_settings.reasoning_effort.clone();
    let client = LlmClient::from_config(LlmConfigOptions {
        driver,
        api_key,
        base_url,
        model,
        max_tokens: runtime_settings.max_tokens,
        thinking_max_tokens: runtime_settings.thinking_max_tokens,
        reasoning: runtime_settings.reasoning,
        reasoning_config: reasoning_config(runtime_settings, resolved_model.model.reasoning),
        openai_config: openai_config(driver, &resolved_model.source_key),
    });

    if let Some(effort) = reasoning_effort {
        client.set_reasoning_effort(Some(effort));
    }

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

fn reasoning_config(
    runtime_settings: &ModelRuntimeSettings,
    model_reasoning: Option<bool>,
) -> Option<ReasoningConfig> {
    runtime_settings
        .reasoning_effort
        .as_ref()
        .map(|effort| ReasoningConfig::Object(serde_json::json!({ "effort": effort })))
        .or_else(|| {
            // thinking_max_tokens > 0 仅当 reasoning 未显式关闭时才生效。
            // 若 model_reasoning == Some(false)，说明用户明确关闭了 thinking，
            // 此时 thinking_max_tokens 仅作为预算上限，不应强制开启 thinking。
            if runtime_settings.thinking_max_tokens > 0 && model_reasoning != Some(false) {
                Some(ReasoningConfig::ThinkingBudget(
                    runtime_settings.thinking_max_tokens,
                ))
            } else {
                model_reasoning.map(ReasoningConfig::Bool)
            }
        })
}

#[cfg(test)]
#[path = "provider_client_tests.rs"]
mod tests;
