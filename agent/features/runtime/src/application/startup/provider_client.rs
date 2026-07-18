use super::model_runtime::ModelRuntimeSettings;
use provider::ReasoningLevel;
use provider::{LlmClient, LlmConfigOptions};
use share::config::models::ResolvedModel;

pub fn resolve_api_key(resolved_model: &ResolvedModel) -> Option<String> {
    non_empty_string(&resolved_model.source_config.api_key)
}

pub fn resolve_base_url(
    cli_base_url: Option<String>,
    resolved_model: &ResolvedModel,
) -> Option<String> {
    cli_base_url.or_else(|| non_empty_string(&resolved_model.source_config.base_url))
}

#[allow(clippy::too_many_arguments)]
pub fn build_llm_client(
    driver: &str,
    api_key: String,
    base_url: Option<String>,
    model: String,
    resolved_model: &ResolvedModel,
    runtime_settings: &ModelRuntimeSettings,
    max_reasoning: Option<&str>,
    timeout_secs: u64,
) -> Result<LlmClient, provider::LlmError> {
    let gateway = provider::wire_provider();
    build_llm_client_with_gateway(
        gateway.as_ref(),
        driver,
        api_key,
        base_url,
        model,
        resolved_model,
        runtime_settings,
        max_reasoning,
        timeout_secs,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn build_llm_client_with_gateway(
    gateway: &dyn provider::LlmProviderGateway,
    driver: &str,
    api_key: String,
    base_url: Option<String>,
    model: String,
    resolved_model: &ResolvedModel,
    runtime_settings: &ModelRuntimeSettings,
    max_reasoning: Option<&str>,
    timeout_secs: u64,
) -> Result<LlmClient, provider::LlmError> {
    let client = gateway.client_from_config(LlmConfigOptions {
        driver: driver.to_string(),
        source_key: resolved_model.source_key.clone(),
        api_style: resolved_model.model.api_style.clone(),
        api_key,
        base_url,
        model,
        max_tokens: runtime_settings.max_tokens,
        reasoning: runtime_settings.reasoning,
        reasoning_config: None,
        timeout_secs,
    })?;

    // Config reasoning 上限已退役；这里只应用模型请求与 provider 能力上限。
    let max_level = max_reasoning.and_then(ReasoningLevel::parse);

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

    client.with_default_reasoning(final_level)
}

fn non_empty_string(value: &str) -> Option<String> {
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

#[cfg(test)]
#[path = "provider_client_tests.rs"]
mod tests;
