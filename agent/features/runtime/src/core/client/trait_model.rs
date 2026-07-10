use sdk::{ModelSummary, SdkError};

use super::accessors::AgentClientImpl;
use crate::core::config_app_service::ConfigAppService;
use crate::core::config_port::ConfigReader;

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

    let svc = ConfigAppService::new(Some(cwd));
    svc.load().await?;
    let snapshot = svc.snapshot().await;

    let runtime_model = snapshot
        .resolve_runtime_model(Some(selection), None)
        .map_err(|e| e.to_string())?;
    let resolved_model = runtime_model.resolved_model().clone();

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
        resolve_model_runtime_settings(runtime_model.max_tokens(), &resolved_model.model, true);

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

pub(super) async fn list_models_impl(me: &AgentClientImpl) -> Result<Vec<ModelSummary>> {
    let svc = ConfigAppService::new(Some(&me.inner.cwd));
    let _ = svc.load().await.map_err(SdkError::Init)?;
    let snapshot = svc.snapshot().await;
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
