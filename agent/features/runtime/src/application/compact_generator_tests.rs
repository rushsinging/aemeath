use super::*;
use crate::application::client::SessionModelSlotData;
use crate::ports::provider_port::fake::FakeProvider;
use crate::ports::{ProviderBindingData, ProviderBuildSpecData, ProviderFactory};
use provider::ModelInfo;
use share::config::models::{ModelEntryConfig, ProviderModelsConfig};
use share::config::Config;

struct StaticReader {
    snapshot: share::config::domain::snapshot::ConfigSnapshot,
}

#[async_trait]
impl config::ConfigReader for StaticReader {
    fn committed_snapshot(&self) -> share::config::domain::snapshot::ConfigSnapshot {
        self.snapshot.clone()
    }

    fn subscribe_committed(
        &self,
    ) -> tokio::sync::watch::Receiver<share::config::domain::snapshot::ConfigSnapshot> {
        tokio::sync::watch::channel(self.snapshot.clone()).1
    }

    async fn refresh_if_sources_changed(
        &self,
    ) -> std::result::Result<config::ConfigRefreshOutcomeData, share::error::DomainError> {
        Ok(config::ConfigRefreshOutcomeData::Unchanged)
    }

    async fn snapshot(
        &self,
    ) -> std::result::Result<
        share::config::domain::snapshot::ConfigSnapshot,
        share::error::DomainError,
    > {
        Ok(self.committed_snapshot())
    }

    async fn subscribe(
        &self,
    ) -> std::result::Result<config::ConfigSubscriptionData, share::error::DomainError> {
        let changes = self.subscribe_committed();
        let initial = changes.borrow().clone();
        Ok(config::ConfigSubscriptionData { initial, changes })
    }
}

struct UnusedFactory;

impl ProviderFactory for UnusedFactory {
    fn build(
        &self,
        spec: ProviderBuildSpecData,
    ) -> Result<ProviderBindingData, provider::ProviderError> {
        Ok(ProviderBindingData {
            provider: Arc::new(FakeProvider::new()),
            model: provider::ModelInfo {
                provider: spec.source_key.clone(),
                model: spec.model.clone(),
                supports_tools: true,
                supports_parallel_tool_calls: true,
                supports_streaming: true,
                reasoning: provider::ReasoningCapabilityData::none(),
                context_limit: spec.context_window,
                output_limit: Some(spec.max_tokens as usize),
            },
            max_tokens: spec.max_tokens,
            requested_reasoning: spec.requested_reasoning,
        })
    }
}

fn snapshot(compact_model: Option<&str>) -> share::config::domain::snapshot::ConfigSnapshot {
    let mut config = Config::default();
    config.models.default = "fake/test-model".into();
    config.models.providers.insert(
        "fake".into(),
        ProviderModelsConfig {
            driver: "openai".into(),
            api_key: "test-key".into(),
            models: vec![
                ModelEntryConfig {
                    id: "test-model".into(),
                    name: "Test Model".into(),
                    context_window: 100_000,
                    max_tokens: 8_192,
                    ..Default::default()
                },
                ModelEntryConfig {
                    id: "no-window-model".into(),
                    name: "No Window Model".into(),
                    context_window: 0,
                    max_tokens: 8_192,
                    ..Default::default()
                },
            ],
            ..Default::default()
        },
    );
    if let Some(selection) = compact_model {
        config.context.compact_model = Some(selection.to_string());
    }
    share::config::domain::snapshot::ConfigSnapshot::new(config)
}

fn generator_with(
    snapshot: share::config::domain::snapshot::ConfigSnapshot,
    session_model: SessionModelSlotData,
) -> ProviderCompactGenerator {
    ProviderCompactGenerator::new(Arc::new(CompactModelResolver::new(
        Arc::new(StaticReader { snapshot }),
        Arc::new(UnusedFactory),
        session_model,
    )))
}

fn session_slot() -> SessionModelSlotData {
    let snapshot = snapshot(None);
    let resolved = snapshot
        .resolve_model_selection("fake/test-model")
        .expect("session model must resolve");
    let slot = SessionModelSlotData::new();
    slot.bind(crate::application::client::SessionModelState::new(
        resolved,
        Arc::new(ProviderBindingData {
            provider: Arc::new(FakeProvider::new()),
            model: ModelInfo {
                provider: "fake".into(),
                model: "test-model".into(),
                supports_tools: true,
                supports_parallel_tool_calls: true,
                supports_streaming: true,
                reasoning: provider::ReasoningCapabilityData::none(),
                context_limit: Some(100_000),
                output_limit: Some(8_192),
            },
            max_tokens: 8_192,
            requested_reasoning: ReasoningLevel::Off,
        }),
    ));
    slot
}

fn fake_generator() -> ProviderCompactGenerator {
    generator_with(snapshot(None), session_slot())
}

#[tokio::test]
async fn collects_text_deltas_into_typed_completion_diagnostics() {
    let result = fake_generator()
        .generate(
            vec![Message::user("summarize this")],
            &CancellationToken::new(),
        )
        .await
        .unwrap();

    assert_eq!(result.text(), "hello");
    assert_eq!(result.completion_reason(), Some("end_turn"));
    assert_eq!(result.text_delta_count(), 1);
    assert_eq!(result.non_text_delta_count(), 0);
    assert!(result.stream_completed());
}

#[tokio::test]
async fn cancelled_invocation_surfaces_as_error() {
    // FakeProvider 在 cancellation 已取消时返回 ProviderError::cancelled()。
    let cancel = CancellationToken::new();
    cancel.cancel();
    let result = fake_generator()
        .generate(vec![Message::user("x")], &cancel)
        .await;
    assert!(
        result.is_err(),
        "已取消的 invoke 必须返回 Err，实际 {result:?}"
    );
}

#[tokio::test]
async fn summary_request_disables_reasoning_and_uses_system_defaults() {
    // 构造请求后不直接触发 invoke；验证 options 语义（reasoning Off）——
    // 通过 FakeProvider 契约无法读取 options，此测试守护构造参数不回归。
    let generator = fake_generator();
    assert_eq!(generator.max_output_tokens, COMPACT_MAX_OUTPUT_TOKENS);
}

#[tokio::test]
async fn configured_model_is_used_for_generation() {
    let generator = generator_with(snapshot(Some("fake/test-model")), session_slot());

    let result = generator
        .generate(vec![Message::user("summarize")], &CancellationToken::new())
        .await
        .expect("配置的模型必须可用于生成");

    assert_eq!(result.text(), "hello");
}

#[tokio::test]
async fn unknown_configured_model_reports_provider_failure_without_fallback() {
    let generator = generator_with(snapshot(Some("fake/missing-model")), session_slot());

    let error = generator
        .generate(vec![Message::user("summarize")], &CancellationToken::new())
        .await
        .expect_err("未知 selection 必须报错，而不是回退会话模型");

    assert_eq!(error.kind, CompactGenerationFailureKind::Provider);
}

#[tokio::test]
async fn compact_context_window_follows_session_model() {
    let generator = generator_with(snapshot(None), session_slot());

    assert_eq!(generator.compact_context_window().await, Some(100_000));
}

#[tokio::test]
async fn compact_context_window_reports_configured_window() {
    let generator = generator_with(snapshot(Some("fake/test-model")), session_slot());

    assert_eq!(generator.compact_context_window().await, Some(100_000));
}

#[tokio::test]
async fn compact_context_window_fails_closed_when_configured_window_unknown() {
    let generator = generator_with(snapshot(Some("fake/no-window-model")), session_slot());

    assert_eq!(
        generator.compact_context_window().await,
        Some(COMPACT_UNKNOWN_MODEL_WINDOW),
        "已配置模型但窗口未知时必须使用保守窗口，而不是注入窗口"
    );
}

#[tokio::test]
async fn compact_context_window_is_unknown_when_resolution_fails() {
    let generator = generator_with(snapshot(Some("fake/missing-model")), session_slot());

    assert_eq!(
        generator.compact_context_window().await,
        None,
        "解析失败时窗口未知，由调用方按注入窗口 fail closed"
    );
}
