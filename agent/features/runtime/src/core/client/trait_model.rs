use sdk::{ModelSummary, SdkError};

use super::accessors::AgentClientImpl;
use crate::core::port::ProviderInfoPort;
use crate::utils::adapter::LlmClientAdapter;
use crate::utils::bootstrap::config_manager::ConfigManager;

type Result<T> = std::result::Result<T, SdkError>;

/// 由 selection 字符串解析配置并构建新 `LlmClient` + `ModelSwitchResult`（#567）。
///
/// 在 loop_runner idle 分支收到 `SwitchModel` 事件时调用。
/// 从 `ConfigManager` 加载配置，经 `resolve_model_selection` 解析 `Provider/Model`，
/// 再构建 `LlmClient`。解析失败返回 `String` 错误信息。
pub(crate) async fn build_llm_client_for_switch(
    selection: &str,
    cwd: &std::path::Path,
) -> std::result::Result<(provider::api::LlmClient, sdk::ModelSwitchResult), String> {
    use crate::utils::bootstrap::{
        build_llm_client, resolve_api_key, resolve_base_url, resolve_model_runtime_settings,
    };
    use provider::api::ProviderDriverKind;

    let config = ConfigManager::new(Some(cwd)).load().await?;

    let resolved_model = config
        .models
        .resolve_model_selection(selection)
        .map_err(|e| e.to_string())?;

    let driver =
        ProviderDriverKind::parse(&resolved_model.driver).unwrap_or(ProviderDriverKind::OpenAI);

    let api_key = resolve_api_key(None, &resolved_model, None).ok_or_else(|| {
        format!(
            "API key 未设置。请为 {} 配置 api_key，或设置对应环境变量。",
            resolved_model.source_key
        )
    })?;

    let base_url = resolve_base_url(None, &resolved_model);
    let model_id = resolved_model.model.id.clone();

    let runtime_settings =
        resolve_model_runtime_settings(None, &resolved_model.model, Some(&config), true)
            .map_err(|e| e.to_string())?;

    let new_client = build_llm_client(
        driver,
        api_key,
        base_url,
        model_id.clone(),
        &resolved_model,
        &runtime_settings,
        None,
    );

    let display_name = if resolved_model.model.name.is_empty() {
        &resolved_model.model.id
    } else {
        &resolved_model.model.name
    };
    let display = format!("{}/{}", resolved_model.source_key, display_name);

    let result = sdk::ModelSwitchResult {
        display_name: display,
        context_window: resolved_model.model.context_window,
        reasoning_active: Some(runtime_settings.reasoning),
    };

    Ok((new_client, result))
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
