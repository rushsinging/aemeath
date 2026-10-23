//! LLM 文本调用端口（#1863）：一次性请求 → 全量文本收口的通用抽象。
//!
//! 供 context（compact 摘要）等域经注入消费，替代各域专用生成器 trait；
//! runtime 侧模型选择策略（resolver）不在本端口内——调用方随请求给定
//! 目标模型。

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

use crate::published_language::{ModelInfo, ProviderRequestData};

/// 一次性 LLM 文本调用的产出（流增量收口后的全量视图）。
#[derive(Debug, Clone)]
pub struct LlmTextOutputData {
    pub text: String,
    pub completion_reason: Option<String>,
    pub text_delta_count: usize,
    pub non_text_delta_count: usize,
    pub stream_completed: bool,
}

/// LLM 文本调用端口：发送请求并收集全部文本增量直到 Stop。
///
/// 语义契约：
/// - 流内 `Error` 帧或提前结束 → `Err(ProviderError)`
/// - 取消经 `request.cancellation` 传递，触发时返回 `ProviderError::cancelled`
#[async_trait]
pub trait LlmTextInvokePort: Send + Sync {
    async fn invoke_text(
        &self,
        model: &ModelInfo,
        request: &ProviderRequestData,
        cancel: &CancellationToken,
    ) -> Result<LlmTextOutputData, crate::ProviderError>;
}

#[async_trait]
impl LlmTextInvokePort for crate::adapters::client::LlmClient {
    async fn invoke_text(
        &self,
        model: &ModelInfo,
        request: &ProviderRequestData,
        cancel: &CancellationToken,
    ) -> Result<LlmTextOutputData, crate::ProviderError> {
        use crate::published_language::{ProviderContentData, ProviderResponseChunk};
        use futures_util::StreamExt;

        let mut stream = self.invoke_stream(model, request).await?;
        let mut text = String::new();
        let mut text_delta_count = 0usize;
        let mut non_text_delta_count = 0usize;
        let _ = cancel; // 取消真相源在 request.cancellation（invoke_stream 单通道消费）。
        while let Some(event) = stream.next().await {
            match event {
                ProviderResponseChunk::Content(ProviderContentData::Text(part)) => {
                    text_delta_count += 1;
                    text.push_str(&part);
                }
                ProviderResponseChunk::Content(_) => non_text_delta_count += 1,
                // Usage 帧不属于内容增量。
                ProviderResponseChunk::Usage(_) => {}
                ProviderResponseChunk::Stop(stop_reason) => {
                    return Ok(LlmTextOutputData {
                        text,
                        completion_reason: Some(stop_reason_label(&stop_reason)),
                        text_delta_count,
                        non_text_delta_count,
                        stream_completed: true,
                    });
                }
                ProviderResponseChunk::Error(error) => return Err(error),
            }
        }
        Err(crate::ProviderError::fatal(
            crate::ProviderErrorKind::UpstreamUnavailable,
            "Provider 流在完成事件前结束",
        ))
    }
}

/// Stop 原因的稳定标签（与调用方日志约定一致）。
fn stop_reason_label(reason: &crate::published_language::ResponseStopReason) -> String {
    use crate::published_language::ResponseStopReason;
    match reason {
        ResponseStopReason::EndTurn => "end_turn".to_string(),
        ResponseStopReason::ToolUse => "tool_use".to_string(),
        ResponseStopReason::MaxOutputTokens => "max_output_tokens".to_string(),
        ResponseStopReason::ContentFiltered => "content_filtered".to_string(),
        ResponseStopReason::StopSequence => "stop_sequence".to_string(),
        ResponseStopReason::Other(other) => format!("other:{other}"),
    }
}
