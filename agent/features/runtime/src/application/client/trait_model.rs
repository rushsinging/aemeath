use sdk::{ModelSummary, SdkError};

use super::accessors::AgentClientImpl;
use crate::ports::{ProviderBuildSpecData, ProviderFactory};
use config::{resolve_provider_runtime, ConfigReader};

type Result<T> = std::result::Result<T, SdkError>;

/// 由 selection 字符串解析配置并通过 ProviderFactory 构建新 `ProviderBindingData`
/// + `ModelSwitchResult`（#567 / #907）。
///
/// 在 loop_runner idle 分支收到 `SwitchModel` 事件时调用。
/// 从 `ConfigReader` 加载配置（gate-aware），经 `resolve_model_selection` 解析
/// `Provider/Model`，再构建 `ProviderBuildSpecData` 交由 factory 构建 binding。
pub(crate) async fn build_provider_binding_for_switch(
    selection: &str,
    query: &dyn ConfigReader,
    factory: &dyn ProviderFactory,
) -> std::result::Result<(crate::ports::ProviderBindingData, sdk::ModelSwitchResult), String> {
    let snapshot = query
        .snapshot()
        .await
        .map_err(|_| "config query unavailable (session switch in progress)".to_string())?;

    let runtime_model = snapshot
        .resolve_runtime_model((!selection.trim().is_empty()).then_some(selection), None)
        .map_err(|e| e.to_string())?;
    build_provider_binding_from_runtime_model(runtime_model, &snapshot, None, factory)
}

pub(crate) fn build_provider_binding_from_runtime_model(
    runtime_model: share::config::models::ResolvedRuntimeModel,
    snapshot: &share::config::domain::snapshot::ConfigSnapshot,
    base_url_override: Option<&str>,
    factory: &dyn ProviderFactory,
) -> std::result::Result<(crate::ports::ProviderBindingData, sdk::ModelSwitchResult), String> {
    let resolved_model = runtime_model.resolved_model().clone();

    let driver = resolved_model.driver.as_str();

    let api_key = non_empty_string(&resolved_model.source_config.api_key).ok_or_else(|| {
        format!(
            "API key 未设置。请为 {} 配置 api_key，或设置对应环境变量。",
            resolved_model.source_key
        )
    })?;

    let runtime_provider = resolve_provider_runtime(snapshot, &resolved_model, base_url_override);
    let base_url = runtime_provider.base_url;
    let model_id = provider::ModelIdData {
        provider: resolved_model.source_key.clone(),
        model: resolved_model.model.id.clone(),
    };

    let requested_reasoning = resolved_model
        .model
        .reasoning_effort
        .as_deref()
        .and_then(share::reasoning::ReasoningLevel::parse)
        .unwrap_or(if resolved_model.model.reasoning.unwrap_or(true) {
            share::reasoning::ReasoningLevel::Medium
        } else {
            share::reasoning::ReasoningLevel::Off
        });

    let spec = ProviderBuildSpecData {
        driver: driver.to_string(),
        source_key: resolved_model.source_key.clone(),
        api_style: resolved_model.model.api_style.clone(),
        api_key,
        base_url,
        model: model_id.clone(),
        max_tokens: runtime_model.max_tokens(),
        requested_reasoning,
        context_window: if resolved_model.model.context_window > 0 {
            Some(resolved_model.model.context_window)
        } else {
            None
        },
        timeout: std::time::Duration::from_secs(snapshot.api_timeout_secs()),
        user_agent: runtime_provider.user_agent,
    };

    let binding = factory.build(spec).map_err(|e| e.to_string())?;

    let display_name = if resolved_model.model.name.is_empty() {
        &resolved_model.model.id
    } else {
        &resolved_model.model.name
    };
    let display = format!("{}/{}", resolved_model.source_key, display_name);

    let result = sdk::ModelSwitchResult {
        display_name: display,
        context_window: resolved_model.model.context_window,
        reasoning_active: Some(requested_reasoning != share::reasoning::ReasoningLevel::Off),
        reasoning_level: Some(requested_reasoning),
    };

    Ok((binding, result))
}

pub(super) async fn list_models_impl(me: &AgentClientImpl) -> Result<Vec<ModelSummary>> {
    let snapshot = me
        .inner
        .shell
        .config_query
        .snapshot()
        .await
        .map_err(|_| SdkError::Internal("config query unavailable".to_string()))?;
    Ok(snapshot
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

fn non_empty_string(value: &str) -> Option<String> {
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

#[cfg(test)]
#[path = "trait_model_tests.rs"]
mod tests;
