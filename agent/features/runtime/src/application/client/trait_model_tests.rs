use super::*;
use config::ConfigReader;
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
impl ConfigReader for CountingQuery {
    fn committed_snapshot(&self) -> ConfigSnapshot {
        self.snapshot.clone()
    }

    fn subscribe_committed(&self) -> tokio::sync::watch::Receiver<ConfigSnapshot> {
        let (sender, receiver) = tokio::sync::watch::channel(self.snapshot.clone());
        let _ = sender; // fake：不广播
        receiver
    }

    async fn refresh_if_sources_changed(
        &self,
    ) -> std::result::Result<config::ConfigRefreshOutcomeData, share::error::DomainError> {
        Ok(config::ConfigRefreshOutcomeData::Unchanged)
    }

    async fn snapshot(&self) -> std::result::Result<ConfigSnapshot, share::error::DomainError> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        Ok(self.snapshot.clone())
    }

    async fn subscribe(
        &self,
    ) -> std::result::Result<config::ConfigSubscriptionData, share::error::DomainError> {
        Err(share::error::DomainError::unavailable(
            "config",
            "配置读取暂不可用",
        ))
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

// Test factory — builds a ProviderBindingData wrapping a pure Fake ProviderPort.
// Does NOT construct a provider client; uses the runtime port's FakeProvider contract.
fn test_factory() -> Arc<dyn ProviderFactory> {
    use crate::ports::provider_port::{
        CancellationSignal, InvocationRequestData, InvocationStreamData, ModelCapabilityData,
        ProviderError, ProviderErrorKind, ReasoningCapabilityData, ReasoningLevel,
    };
    use crate::ports::ProviderPort as ProviderPortTrait;

    struct TestPort {
        capabilities: std::collections::HashMap<provider::ModelIdData, ModelCapabilityData>,
    }

    #[async_trait::async_trait]
    impl ProviderPortTrait for TestPort {
        fn capabilities(
            &self,
            model: &provider::ModelIdData,
        ) -> std::result::Result<ModelCapabilityData, ProviderError> {
            self.capabilities.get(model).cloned().ok_or_else(|| {
                ProviderError::fatal(
                    ProviderErrorKind::ModelUnavailable,
                    format!("unknown model: {model}"),
                )
            })
        }

        async fn invoke(
            &self,
            _request: InvocationRequestData,
            _cancellation: &dyn CancellationSignal,
        ) -> std::result::Result<InvocationStreamData, ProviderError> {
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
            spec: ProviderBuildSpecData,
        ) -> std::result::Result<crate::ports::ProviderBindingData, ProviderError> {
            let capability = ModelCapabilityData {
                model: spec.model.clone(),
                supports_tools: true,
                supports_parallel_tool_calls: true,
                supports_streaming: true,
                reasoning: ReasoningCapabilityData::new(vec![
                    ReasoningLevel::Off,
                    ReasoningLevel::Low,
                    ReasoningLevel::Medium,
                ])
                .unwrap_or_else(|_| ReasoningCapabilityData::none()),
                context_limit: spec.context_window,
                output_limit: Some(spec.max_tokens as usize),
            };
            let capabilities = std::collections::HashMap::from([(spec.model.clone(), capability)]);
            let port: Arc<dyn ProviderPortTrait> = Arc::new(TestPort { capabilities });
            Ok(crate::ports::ProviderBindingData {
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
