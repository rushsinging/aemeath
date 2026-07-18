use sdk::{ModelSummary, SdkError};

use super::accessors::AgentClientImpl;
use config::ConfigQuery;

type Result<T> = std::result::Result<T, SdkError>;

/// 由 selection 字符串解析配置并构建新 `LlmClient` + `ModelSwitchResult`（#567）。
///
/// 在 loop_runner idle 分支收到 `SwitchModel` 事件时调用。
/// 从 `ConfigQuery` 加载配置（gate-aware），经 `resolve_model_selection` 解析
/// `Provider/Model`，再构建 `LlmClient`。解析失败返回 `String` 错误信息。
pub(crate) async fn build_llm_client_for_switch(
    selection: &str,
    query: &dyn ConfigQuery,
) -> std::result::Result<(provider::LlmClient, sdk::ModelSwitchResult), String> {
    use crate::application::startup::{
        build_llm_client, resolve_api_key, resolve_base_url, resolve_model_runtime_settings,
    };
    let snapshot = query
        .snapshot()
        .await
        .map_err(|_| "config query unavailable (session switch in progress)".to_string())?;

    let runtime_model = snapshot
        .resolve_runtime_model(Some(selection), None)
        .map_err(|e| e.to_string())?;
    let resolved_model = runtime_model.resolved_model().clone();

    let driver = resolved_model.driver.as_str();

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
        provider::DEFAULT_TIMEOUT_SECS,
    )
    .map_err(|error| error.to_string())?;

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
    let snapshot = me
        .inner
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

#[cfg(test)]
mod tests {
    use super::*;
    use config::{ConfigQuery, ConfigQueryError};
    use share::config::domain::snapshot::ConfigSnapshot;
    use share::config::models::{ModelEntryConfig, ProviderModelsConfig};
    use share::config::Config;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct CountingQuery {
        reads: AtomicUsize,
        snapshot: ConfigSnapshot,
    }

    #[async_trait::async_trait]
    impl ConfigQuery for CountingQuery {
        async fn snapshot(&self) -> std::result::Result<ConfigSnapshot, ConfigQueryError> {
            self.reads.fetch_add(1, Ordering::SeqCst);
            Ok(self.snapshot.clone())
        }

        async fn subscribe(
            &self,
        ) -> std::result::Result<config::ConfigSubscription, ConfigQueryError> {
            Err(ConfigQueryError::Unavailable)
        }
    }

    fn query() -> CountingQuery {
        let mut config = Config::default();
        config.models.default = "local/test-model".into();
        config.models.providers.insert(
            "local".into(),
            ProviderModelsConfig {
                driver: "openai".into(),
                api_key: "test-key".into(),
                models: vec![ModelEntryConfig {
                    id: "test-model".into(),
                    name: "Test Model".into(),
                    context_window: 8192,
                    max_tokens: 1024,
                    ..Default::default()
                }],
                ..Default::default()
            },
        );
        CountingQuery {
            reads: AtomicUsize::new(0),
            snapshot: ConfigSnapshot::new(config),
        }
    }

    #[tokio::test]
    async fn model_switch_reads_injected_snapshot_once() {
        let query = query();
        let (_, result) = build_llm_client_for_switch("local/test-model", &query)
            .await
            .unwrap();
        assert_eq!(result.display_name, "local/Test Model");
        assert_eq!(query.reads.load(Ordering::SeqCst), 1);
    }
}
