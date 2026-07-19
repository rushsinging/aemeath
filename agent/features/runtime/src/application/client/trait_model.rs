use sdk::{ModelSummary, SdkError};

use super::accessors::AgentClientImpl;
use crate::ports::{ProviderBuildSpec, ProviderFactory};
use config::ConfigQuery;

type Result<T> = std::result::Result<T, SdkError>;

/// 由 selection 字符串解析配置并通过 ProviderFactory 构建新 `ProviderBinding`
/// + `ModelSwitchResult`（#567 / #907）。
///
/// 在 loop_runner idle 分支收到 `SwitchModel` 事件时调用。
/// 从 `ConfigQuery` 加载配置（gate-aware），经 `resolve_model_selection` 解析
/// `Provider/Model`，再构建 `ProviderBuildSpec` 交由 factory 构建 binding。
pub(crate) async fn build_provider_binding_for_switch(
    selection: &str,
    query: &dyn ConfigQuery,
    factory: &dyn ProviderFactory,
) -> std::result::Result<(crate::ports::ProviderBinding, sdk::ModelSwitchResult), String> {
    let snapshot = query
        .snapshot()
        .await
        .map_err(|_| "config query unavailable (session switch in progress)".to_string())?;

    let runtime_model = snapshot
        .resolve_runtime_model(Some(selection), None)
        .map_err(|e| e.to_string())?;
    let resolved_model = runtime_model.resolved_model().clone();

    let driver = resolved_model.driver.as_str();

    let api_key = non_empty_string(&resolved_model.source_config.api_key).ok_or_else(|| {
        format!(
            "API key 未设置。请为 {} 配置 api_key，或设置对应环境变量。",
            resolved_model.source_key
        )
    })?;

    let base_url = non_empty_string(&resolved_model.source_config.base_url);
    let model_id = provider::ModelId {
        provider: resolved_model.source_key.clone(),
        model: resolved_model.model.id.clone(),
    };

    let requested_reasoning = resolved_model
        .model
        .reasoning_effort
        .as_deref()
        .and_then(provider::ReasoningLevel::parse)
        .unwrap_or(if resolved_model.model.reasoning.unwrap_or(true) {
            provider::ReasoningLevel::Medium
        } else {
            provider::ReasoningLevel::Off
        });

    let spec = ProviderBuildSpec {
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
        timeout: std::time::Duration::from_secs(provider::DEFAULT_TIMEOUT_SECS),
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
        reasoning_active: Some(requested_reasoning != provider::ReasoningLevel::Off),
    };

    Ok((binding, result))
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

fn non_empty_string(value: &str) -> Option<String> {
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use config::{ConfigQuery, ConfigQueryError};
    use share::config::domain::snapshot::ConfigSnapshot;
    use share::config::models::{ModelEntryConfig, ProviderModelsConfig};
    use share::config::Config;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

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
        let factory = test_factory();
        let (_, result) =
            build_provider_binding_for_switch("local/test-model", &query, factory.as_ref())
                .await
                .unwrap();
        assert_eq!(result.display_name, "local/Test Model");
        assert_eq!(query.reads.load(Ordering::SeqCst), 1);
    }

    // Test factory — builds a ProviderBinding wrapping a pure Fake ProviderPort.
    // Does NOT construct a provider client; uses the runtime port's FakeProvider contract.
    fn test_factory() -> Arc<dyn ProviderFactory> {
        use crate::ports::provider_port::{
            CancellationSignal, InvocationRequest, InvocationStream, ModelCapability,
            ProviderError, ProviderErrorKind, ReasoningCapability, ReasoningLevel,
            ReasoningMappingKind,
        };
        use crate::ports::ProviderPort as ProviderPortTrait;

        struct TestPort {
            capabilities: std::collections::HashMap<provider::ModelId, ModelCapability>,
        }

        #[async_trait::async_trait]
        impl ProviderPortTrait for TestPort {
            fn capabilities(
                &self,
                model: &provider::ModelId,
            ) -> std::result::Result<ModelCapability, ProviderError> {
                self.capabilities.get(model).cloned().ok_or_else(|| {
                    ProviderError::fatal(
                        ProviderErrorKind::ModelUnavailable,
                        format!("unknown model: {model}"),
                    )
                })
            }

            async fn invoke(
                &self,
                _request: InvocationRequest,
                _cancellation: &dyn CancellationSignal,
            ) -> std::result::Result<InvocationStream, ProviderError> {
                Err(ProviderError::fatal(
                    ProviderErrorKind::UpstreamUnavailable,
                    "test provider does not support invocation",
                ))
            }
        }

        struct TestFactory;
        impl ProviderFactory for TestFactory {
            fn build(
                &self,
                spec: ProviderBuildSpec,
            ) -> std::result::Result<crate::ports::ProviderBinding, ProviderError> {
                let capability = ModelCapability {
                    model: spec.model.clone(),
                    supports_tools: true,
                    supports_parallel_tool_calls: true,
                    supports_streaming: true,
                    reasoning: ReasoningCapability::new(
                        vec![
                            ReasoningLevel::Off,
                            ReasoningLevel::Low,
                            ReasoningLevel::Medium,
                        ],
                        ReasoningMappingKind::Effort,
                    )
                    .unwrap_or_else(|_| ReasoningCapability::none()),
                    context_limit: spec.context_window,
                    output_limit: Some(spec.max_tokens as usize),
                };
                let capabilities =
                    std::collections::HashMap::from([(spec.model.clone(), capability)]);
                let port: Arc<dyn ProviderPortTrait> = Arc::new(TestPort { capabilities });
                Ok(crate::ports::ProviderBinding {
                    provider: port,
                    model: spec.model,
                    max_tokens: spec.max_tokens,
                    requested_reasoning: spec.requested_reasoning,
                    context_window: spec.context_window,
                })
            }
        }

        Arc::new(TestFactory)
    }
}
