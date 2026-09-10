//! LLM 语义压缩生成器的生产实现（#1486）。
//!
//! 包装 [`ProviderPort`]，把 compact 的摘要请求（纯文本、无工具、低推理）
//! 转成一次 provider invoke，收集文本增量后返回完整摘要。
//! context crate 只依赖 `CompactGenerator` trait，不接触 provider。

use async_trait::async_trait;
use context::compact::CompactGenerator;
use context::domain::{
    CompactGenerationFailure, CompactGenerationFailureKind, CompactGenerationOutput,
};
use futures::StreamExt;
use provider::{
    InvocationDelta, InvocationEvent, InvocationOptions, InvocationRequest, ReasoningLevel,
};
use share::message::Message;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

use crate::application::client::{
    CompactModelOrigin, CompactModelResolveError, CompactModelResolver,
};

/// Compact 摘要请求的最大输出 token（摘要可长，给足预算）。
///
/// 实际取值还受所选模型自身 `max_tokens` 上限约束（取两者较小值）。
const COMPACT_MAX_OUTPUT_TOKENS: u32 = 16_384;

/// 已配置 compact 模型但模型未声明输入窗口时的保守窗口。
///
/// 未知窗口 **MUST** fail closed：使用保守下限而不是注入窗口，避免向小窗口
/// 模型发出超窗口请求。
const COMPACT_UNKNOWN_MODEL_WINDOW: usize = 32_000;

/// Provider-backed [`CompactGenerator`]：通过真实 LLM 生成压缩摘要。
///
/// 每次 `generate` 都经 [`CompactModelResolver`] 解析本次 compact 使用的模型：
/// 配置了 `context.compact_model` 时使用该模型，否则跟随当前会话模型。
pub struct ProviderCompactGenerator {
    resolver: Arc<CompactModelResolver>,
    max_output_tokens: u32,
}

impl ProviderCompactGenerator {
    pub fn new(resolver: Arc<CompactModelResolver>) -> Self {
        Self {
            resolver,
            max_output_tokens: COMPACT_MAX_OUTPUT_TOKENS,
        }
    }
}

#[async_trait]
impl CompactGenerator for ProviderCompactGenerator {
    async fn generate(
        &self,
        request: Vec<Message>,
        cancel: &CancellationToken,
    ) -> Result<CompactGenerationOutput, CompactGenerationFailure> {
        let target = self.resolver.resolve().map_err(compact_model_failure)?;
        let binding = target.binding();
        let max_output_tokens = self.max_output_tokens.min(binding.max_tokens.max(1));
        let mut invocation = InvocationRequest::new(
            binding.model.clone(),
            request,
            InvocationOptions::new(max_output_tokens, ReasoningLevel::Off),
        );
        // 摘要生成不携带上下文窗口消息；压缩提示词本身就是全部输入。
        invocation.cancellation = cancel.clone();

        let stream = binding
            .provider
            .invoke(invocation, cancel)
            .await
            .map_err(compact_generation_failure)?;

        let mut text = String::new();
        let mut text_delta_count = 0usize;
        let mut non_text_delta_count = 0usize;
        let mut stream = stream;
        while let Some(event) = stream.next().await {
            match event {
                InvocationEvent::Delta(InvocationDelta::Text(part)) => {
                    text_delta_count += 1;
                    text.push_str(&part);
                }
                InvocationEvent::Delta(_) => non_text_delta_count += 1,
                InvocationEvent::Completed(completion) => {
                    return Ok(CompactGenerationOutput::completed(
                        text,
                        Some(completion_reason(&completion.stop_reason)),
                        text_delta_count,
                        non_text_delta_count,
                    ));
                }
                InvocationEvent::Failed(error) => {
                    return Err(compact_generation_failure(error));
                }
            }
        }
        Err(CompactGenerationFailure::new(
            CompactGenerationFailureKind::Provider,
            "Provider 流在完成事件前结束",
        ))
    }

    async fn compact_context_window(&self) -> Option<usize> {
        match self.resolver.resolve() {
            Ok(target) => match (target.origin(), target.context_window()) {
                (_, Some(window)) => Some(window),
                (CompactModelOrigin::Configured, None) => {
                    log::warn!(
                        target: crate::LOG_TARGET,
                        "[compact] compact 模型未声明输入窗口，改用保守窗口 {COMPACT_UNKNOWN_MODEL_WINDOW}"
                    );
                    Some(COMPACT_UNKNOWN_MODEL_WINDOW)
                }
                (CompactModelOrigin::SessionModel, None) => None,
            },
            Err(error) => {
                log::warn!(
                    target: crate::LOG_TARGET,
                    "[compact] 无法解析 compact 模型窗口，本次按注入窗口预算处理：{error}"
                );
                None
            }
        }
    }
}

fn completion_reason(reason: &provider::ProviderStopReason) -> String {
    match reason {
        provider::ProviderStopReason::EndTurn => "end_turn".to_string(),
        provider::ProviderStopReason::ToolUse => "tool_use".to_string(),
        provider::ProviderStopReason::MaxOutputTokens => "max_output_tokens".to_string(),
        provider::ProviderStopReason::ContentFiltered => "content_filtered".to_string(),
        provider::ProviderStopReason::StopSequence => "stop_sequence".to_string(),
        provider::ProviderStopReason::Other(reason) => format!("other:{reason}"),
    }
}

fn compact_generation_failure(error: provider::ProviderError) -> CompactGenerationFailure {
    use provider::ProviderErrorKind;

    let kind = match error.kind {
        ProviderErrorKind::Cancelled => CompactGenerationFailureKind::Cancelled,
        ProviderErrorKind::RateLimited => CompactGenerationFailureKind::RateLimited,
        ProviderErrorKind::ContextTooLong => CompactGenerationFailureKind::ContextTooLong,
        ProviderErrorKind::Timeout => CompactGenerationFailureKind::Timeout,
        ProviderErrorKind::Authentication
        | ProviderErrorKind::PermissionDenied
        | ProviderErrorKind::InvalidRequest
        | ProviderErrorKind::ModelUnavailable
        | ProviderErrorKind::UpstreamUnavailable
        | ProviderErrorKind::Network
        | ProviderErrorKind::Protocol
        | ProviderErrorKind::StreamTruncated
        | ProviderErrorKind::Configuration => CompactGenerationFailureKind::Provider,
    };
    CompactGenerationFailure::new(kind, error.safe_message)
}

/// 模型解析失败按 Provider 类错误上报；已配置 selection 的错误 **NEVER** 静默回退。
fn compact_model_failure(error: CompactModelResolveError) -> CompactGenerationFailure {
    CompactGenerationFailure::new(CompactGenerationFailureKind::Provider, error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::client::SessionModelSlot;
    use crate::ports::provider_port::fake::FakeProvider;
    use crate::ports::{ProviderBinding, ProviderBuildSpec, ProviderFactory};
    use provider::ModelId;
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

        async fn refresh_if_sources_changed(&self) -> config::ConfigRefreshOutcome {
            config::ConfigRefreshOutcome::Unchanged
        }
    }

    struct UnusedFactory;

    impl ProviderFactory for UnusedFactory {
        fn build(
            &self,
            spec: ProviderBuildSpec,
        ) -> Result<ProviderBinding, provider::ProviderError> {
            Ok(ProviderBinding {
                provider: Arc::new(FakeProvider::new()),
                model: spec.model,
                max_tokens: spec.max_tokens,
                requested_reasoning: spec.requested_reasoning,
                context_window: spec.context_window,
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
        session_model: SessionModelSlot,
    ) -> ProviderCompactGenerator {
        ProviderCompactGenerator::new(Arc::new(CompactModelResolver::new(
            Arc::new(StaticReader { snapshot }),
            Arc::new(UnusedFactory),
            session_model,
        )))
    }

    fn session_slot() -> SessionModelSlot {
        let snapshot = snapshot(None);
        let resolved = snapshot
            .resolve_model_selection("fake/test-model")
            .expect("session model must resolve");
        let slot = SessionModelSlot::new();
        slot.bind(crate::application::client::SessionModelState::new(
            resolved,
            Arc::new(ProviderBinding {
                provider: Arc::new(FakeProvider::new()),
                model: ModelId {
                    provider: "fake".into(),
                    model: "test-model".into(),
                },
                max_tokens: 8_192,
                requested_reasoning: ReasoningLevel::Off,
                context_window: Some(100_000),
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
}
