use std::sync::Arc;

use sdk::{ModelSummary, SdkError};

use super::accessors::AgentClientImpl;
use crate::core::port::ProviderInfoPort;
use crate::utils::adapter::LlmClientAdapter;
use crate::utils::bootstrap::config_manager::ConfigManager;

type Result<T> = std::result::Result<T, SdkError>;

pub(super) async fn switch_model_impl(
    me: &AgentClientImpl,
    params: sdk::ModelSwitchParams,
) -> Result<sdk::ModelSwitchResult> {
    let (new_client, result) = build_llm_client_for_switch(params);
    *me.inner.current_client.write().unwrap() = Arc::new(new_client);
    Ok(result)
}

/// 由 `ModelSwitchParams` 构建新 `LlmClient` + `ModelSwitchResult`。
///
/// 提取为独立函数供 `switch_model_impl`（RPC 路径）和 loop_runner idle 分支
/// （事件流路径，#497）共用，DRY。
pub(crate) fn build_llm_client_for_switch(
    params: sdk::ModelSwitchParams,
) -> (provider::api::LlmClient, sdk::ModelSwitchResult) {
    use provider::api::openai_compatible::ReasoningConfig;
    use provider::api::ProviderDriverKind;

    let driver = ProviderDriverKind::parse(&params.driver).unwrap_or(ProviderDriverKind::OpenAI);
    let openai_config = switch_model_openai_config(driver, &params.provider_name);

    let reasoning = params.reasoning.unwrap_or(true);
    let reasoning_config = Some(ReasoningConfig::Bool(reasoning));

    let new_client = provider::api::LlmClient::from_config(provider::api::LlmConfigOptions {
        driver,
        api_key: params.api_key,
        base_url: Some(params.base_url),
        model: params.model_id.clone(),
        max_tokens: params.max_tokens,
        reasoning,
        reasoning_config,
        openai_config,
    });

    let display_name = if params.model_name.is_empty() {
        &params.model_id
    } else {
        &params.model_name
    };
    let display = format!("{}/{}", params.provider_name, display_name);

    let result = sdk::ModelSwitchResult {
        display_name: display,
        context_window: params.context_window,
        reasoning_active: Some(reasoning),
    };

    (new_client, result)
}

pub(super) async fn set_thinking_impl(me: &AgentClientImpl, desired: Option<bool>) -> Result<bool> {
    let client = me.inner.current_client.read().unwrap().clone();
    let adapter = LlmClientAdapter::new(client);
    let current = adapter.current_reasoning_level();
    let new_state = desired.unwrap_or(matches!(current, provider::contract::ReasoningLevel::Off));
    let level = if new_state {
        provider::contract::ReasoningLevel::Medium
    } else {
        provider::contract::ReasoningLevel::Off
    };
    adapter.set_reasoning_level(level);
    Ok(new_state)
}

pub(super) async fn get_thinking_impl(me: &AgentClientImpl) -> Result<bool> {
    let client = me.inner.current_client.read().unwrap().clone();
    let adapter = LlmClientAdapter::new(client);
    Ok(!matches!(
        adapter.current_reasoning_level(),
        provider::contract::ReasoningLevel::Off
    ))
}

pub(super) async fn list_models_impl(me: &AgentClientImpl) -> Result<Vec<ModelSummary>> {
    let config = ConfigManager::new(Some(&me.inner.cwd))
        .load()
        .await
        .map_err(SdkError::Init)?;
    Ok(config
        .models
        .list_models()
        .into_iter()
        .map(|(provider, model)| ModelSummary {
            provider,
            id: model.id,
            name: model.name,
            context_window: model.context_window,
            max_tokens: model.max_tokens,
        })
        .collect())
}

fn switch_model_openai_config(
    driver: provider::api::ProviderDriverKind,
    source_key: &str,
) -> Option<provider::api::OpenAIProviderConfig> {
    if matches!(
        driver,
        provider::api::ProviderDriverKind::Anthropic | provider::api::ProviderDriverKind::Ollama
    ) {
        None
    } else {
        Some(provider::api::OpenAIProviderConfig::from_driver(
            driver, source_key,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_switch_model_openai_config_skips_ollama() {
        let result =
            switch_model_openai_config(provider::api::ProviderDriverKind::Ollama, "ollama");

        assert!(result.is_none());
    }

    #[test]
    fn test_switch_model_openai_config_skips_anthropic() {
        let result =
            switch_model_openai_config(provider::api::ProviderDriverKind::Anthropic, "anthropic");

        assert!(result.is_none());
    }

    #[test]
    fn test_switch_model_openai_config_uses_source_key_for_openai_compatible() {
        let result = switch_model_openai_config(provider::api::ProviderDriverKind::Zhipu, "Zhipu")
            .expect("zhipu should use openai-compatible config");

        assert_eq!(result.source_key, "Zhipu");
        assert_eq!(result.driver, provider::api::ProviderDriverKind::Zhipu);
    }
}
