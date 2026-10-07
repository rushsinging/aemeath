//! LLM 语义压缩生成器的生产实现（#1486）。
//!
//! 包装 [`ProviderPort`]，把 compact 的摘要请求（纯文本、无工具、低推理）
//! 转成一次 provider invoke，收集文本增量后返回完整摘要。
//! context crate 只依赖 `CompactGenerator` trait，不接触 provider。

use async_trait::async_trait;
use context::compact::CompactGenerator;
use context::{
    CompactGenerationFailureData, CompactGenerationFailureKind, CompactGenerationOutputData,
};
use futures::StreamExt;
use provider::{InvocationRequestData, ProviderContentData, ProviderResponseChunk};
use share::message::Message;
use share::reasoning::ReasoningLevel;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

use crate::application::client::{
    CompactModelOrigin, CompactModelResolveError, CompactModelResolver,
};

use crate::application::constants::{COMPACT_MAX_OUTPUT_TOKENS, COMPACT_UNKNOWN_MODEL_WINDOW};

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
    ) -> Result<CompactGenerationOutputData, CompactGenerationFailureData> {
        let target = self.resolver.resolve().map_err(compact_model_failure)?;
        let binding = target.binding();
        let max_output_tokens = self.max_output_tokens.min(binding.max_tokens.max(1));
        let mut invocation = InvocationRequestData::new(
            binding.model.clone(),
            request,
            max_output_tokens,
            ReasoningLevel::Off,
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
                ProviderResponseChunk::Content(ProviderContentData::Text(part)) => {
                    text_delta_count += 1;
                    text.push_str(&part);
                }
                ProviderResponseChunk::Content(_) => non_text_delta_count += 1,
                // Usage 帧不属于内容增量（原 completion.usage 不计数）。
                ProviderResponseChunk::Usage(_) => {}
                ProviderResponseChunk::Stop(stop_reason) => {
                    return Ok(CompactGenerationOutputData::completed(
                        text,
                        Some(completion_reason(&stop_reason)),
                        text_delta_count,
                        non_text_delta_count,
                    ));
                }
                ProviderResponseChunk::Error(error) => {
                    return Err(compact_generation_failure(error));
                }
            }
        }
        Err(CompactGenerationFailureData::new(
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

fn completion_reason(reason: &provider::ProviderStopReasonData) -> String {
    match reason {
        provider::ProviderStopReasonData::EndTurn => "end_turn".to_string(),
        provider::ProviderStopReasonData::ToolUse => "tool_use".to_string(),
        provider::ProviderStopReasonData::MaxOutputTokens => "max_output_tokens".to_string(),
        provider::ProviderStopReasonData::ContentFiltered => "content_filtered".to_string(),
        provider::ProviderStopReasonData::StopSequence => "stop_sequence".to_string(),
        provider::ProviderStopReasonData::Other(reason) => format!("other:{reason}"),
    }
}

fn compact_generation_failure(error: provider::ProviderError) -> CompactGenerationFailureData {
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
    CompactGenerationFailureData::new(kind, error.safe_message)
}

/// 模型解析失败按 Provider 类错误上报；已配置 selection 的错误 **NEVER** 静默回退。
fn compact_model_failure(error: CompactModelResolveError) -> CompactGenerationFailureData {
    CompactGenerationFailureData::new(CompactGenerationFailureKind::Provider, error.to_string())
}

#[cfg(test)]
#[path = "compact_generator_tests.rs"]
mod tests;
