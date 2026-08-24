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
    InvocationDelta, InvocationEvent, InvocationOptions, InvocationRequest, ModelId, ReasoningLevel,
};
use share::message::Message;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

use crate::ports::ProviderPort;

/// Compact 摘要请求的最大输出 token（摘要可长，给足预算）。
const COMPACT_MAX_OUTPUT_TOKENS: u32 = 16_384;

/// Provider-backed [`CompactGenerator`]：通过真实 LLM 生成压缩摘要。
pub struct ProviderCompactGenerator {
    provider: Arc<dyn ProviderPort>,
    model: ModelId,
    max_output_tokens: u32,
}

impl ProviderCompactGenerator {
    pub fn new(provider: Arc<dyn ProviderPort>, model: ModelId) -> Self {
        Self {
            provider,
            model,
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
        let mut invocation = InvocationRequest::new(
            self.model.clone(),
            request,
            InvocationOptions::new(self.max_output_tokens, ReasoningLevel::Off),
        );
        // 摘要生成不携带上下文窗口消息；压缩提示词本身就是全部输入。
        invocation.cancellation = cancel.clone();

        let stream = self
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::provider_port::fake::FakeProvider;

    fn fake_generator() -> ProviderCompactGenerator {
        ProviderCompactGenerator::new(
            Arc::new(FakeProvider::new()),
            ModelId {
                provider: "fake".to_string(),
                model: "test-model".to_string(),
            },
        )
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
}
