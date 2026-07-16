use super::model_runtime::ModelRuntimeSettings;
use provider::ProviderDriverKind;
use provider::ReasoningLevel;
use provider::{LlmClient, LlmConfigOptions, OpenAIProviderConfig};
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

#[allow(clippy::too_many_arguments)]
pub fn build_llm_client(
    driver: ProviderDriverKind,
    api_key: String,
    base_url: Option<String>,
    model: String,
    resolved_model: &ResolvedModel,
    runtime_settings: &ModelRuntimeSettings,
    max_reasoning: Option<&str>,
    timeout_secs: u64,
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
        timeout_secs,
    });

    // CLI / env 指定的上限（优先级 CLI > env），clamp 到 provider 能力上限。
    let max_level = max_reasoning.and_then(ReasoningLevel::parse).or_else(|| {
        std::env::var("AEMEATH_MAX_REASONING")
            .ok()
            .and_then(|s| ReasoningLevel::parse(&s))
    });

    // 期望档位来源优先级：
    // 1) 模型配置的 reasoning_effort（显式档位，视为开启思考）
    // 2) reasoning: bool → Medium / Off
    // 无论走哪条，最终都会被 max_level 上限与 provider 能力上限 clamp。
    let desired = match runtime_settings
        .reasoning_effort
        .as_deref()
        .and_then(ReasoningLevel::parse)
    {
        Some(level) => level,
        None => {
            if runtime_settings.reasoning {
                ReasoningLevel::Medium
            } else {
                ReasoningLevel::Off
            }
        }
    };
    let final_level = match max_level {
        Some(cap) => desired.min(cap).min(client.max_reasoning_level()),
        None => desired.min(client.max_reasoning_level()),
    };
    client.set_reasoning_level(final_level);

    client
}

fn provider_driver_api_key_from_env(
    driver: ProviderDriverKind,
    env_value: EnvReader<'_>,
) -> Option<String> {
    share::config::domain::driver_env::driver_api_key_env_name(driver.as_str())
        .and_then(|name| env_or_runtime(name, env_value))
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
