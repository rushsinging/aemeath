//! LLM 调用端口的测试实现面（#2146 收口）。
//!
//! 架构规则：`LlmInvokePort` 的**唯一实现**是 provider 的生产 `LlmClient`；
//! 除 provider crate 外，任何 crate **NEVER** 实现该 trait（含测试 fake）。
//! 测试需要可控 LLM 行为时，用本模块的脚本化实现 [`scripted_llm`] 与
//! 便捷流构造：[`text_stream`] / [`empty_text_stream`] / [`error_stream`] /
//! [`stream_from_chunks`]——脚本即闭包，可自由捕获计数器、通道、录制器等状态。
//!
//! 跨 crate 测试可见（`feature = "dev"`）；生产构建不编译。

use std::sync::Arc;

use async_trait::async_trait;

use crate::published_language::{
    LlmInvokePort, ModelInfo, ProviderContentData, ProviderError, ProviderRequestData,
    ProviderResponseChunk, ProviderResponseStream, ResponseStopReason,
};

/// 测试脚本：按请求产生片段流或错误。
pub type LlmScript =
    dyn Fn(&ProviderRequestData) -> Result<ProviderResponseStream, ProviderError> + Send + Sync;

/// 脚本化 LLM 调用端口——测试侧唯一允许的端口实现。
pub struct ScriptedLlmInvokePort {
    script: Arc<LlmScript>,
}

impl ScriptedLlmInvokePort {
    /// 用脚本构造（脚本返回流或错误；可捕获任意共享状态）。
    pub fn new(
        script: impl Fn(&ProviderRequestData) -> Result<ProviderResponseStream, ProviderError>
            + Send
            + Sync
            + 'static,
    ) -> Self {
        Self {
            script: Arc::new(script),
        }
    }
}

#[async_trait]
impl LlmInvokePort for ScriptedLlmInvokePort {
    async fn invoke_stream(
        &self,
        _model: &ModelInfo,
        request: &ProviderRequestData,
    ) -> Result<ProviderResponseStream, ProviderError> {
        // 回放器不做取消前置检查：取消语义由脚本自行表达（生产 `LlmClient`
        // 才承担 fast-path；测试若被强加拦截会掩盖脚本意图）。
        (self.script)(request)
    }
}

/// 便捷构造：脚本 → `Arc<dyn LlmInvokePort>`（可直接填入
/// `ProviderBindingData.provider`)
///
/// 示例：
/// ```ignore
/// let llm = scripted_llm(move |request| {
///     let text = request.messages.first().map(|m| m.text_content()).unwrap_or_default();
///     Ok(text_stream(&summarize(&text)))
/// });
/// ```
pub fn scripted_llm(
    script: impl Fn(&ProviderRequestData) -> Result<ProviderResponseStream, ProviderError>
        + Send
        + Sync
        + 'static,
) -> Arc<dyn LlmInvokePort> {
    Arc::new(ScriptedLlmInvokePort::new(script))
}

/// 异步脚本：需要 `await`（阻塞等待取消等）时使用。
pub type AsyncLlmScript = dyn for<'a> Fn(
        &'a ProviderRequestData,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<ProviderResponseStream, ProviderError>>
                + Send
                + 'a,
        >,
    > + Send
    + Sync;

/// 异步脚本化端口（阻塞/取消等待场景）。
pub struct AsyncScriptedLlmInvokePort {
    script: Arc<AsyncLlmScript>,
}

#[async_trait]
impl LlmInvokePort for AsyncScriptedLlmInvokePort {
    async fn invoke_stream(
        &self,
        _model: &ModelInfo,
        request: &ProviderRequestData,
    ) -> Result<ProviderResponseStream, ProviderError> {
        // 回放器不做取消前置检查（同同步版说明）。
        (self.script)(request).await
    }
}

/// 便捷构造：异步脚本 → `Arc<dyn LlmInvokePort>`。
pub fn scripted_llm_async(
    script: impl for<'a> Fn(
            &'a ProviderRequestData,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<Output = Result<ProviderResponseStream, ProviderError>>
                    + Send
                    + 'a,
            >,
        > + Send
        + Sync
        + 'static,
) -> Arc<dyn LlmInvokePort> {
    Arc::new(AsyncScriptedLlmInvokePort {
        script: Arc::new(script),
    })
}

/// 固定文本响应（单 Content 帧 + Stop(EndTurn)）。
pub fn fixed_text(text: impl Into<String>) -> Arc<dyn LlmInvokePort> {
    let text = text.into();
    scripted_llm(move |_request| Ok(text_stream(&text)))
}

/// 单 Content 文本帧 + Stop 帧。
pub fn text_stream(text: &str) -> ProviderResponseStream {
    let mut chunks: Vec<ProviderResponseChunk> = Vec::new();
    if !text.is_empty() {
        chunks.push(ProviderResponseChunk::Content(ProviderContentData::Text(
            text.to_string(),
        )));
    }
    chunks.push(ProviderResponseChunk::Stop(ResponseStopReason::EndTurn));
    stream_from_chunks(chunks)
}

/// 文本 + Usage 帧 + Stop 帧（需要计量断言的测试使用）。
pub fn text_stream_with_usage(
    text: &str,
    input_tokens: u32,
    output_tokens: u32,
) -> ProviderResponseStream {
    use crate::published_language::TokenUsageData;
    let mut chunks: Vec<ProviderResponseChunk> = Vec::new();
    if !text.is_empty() {
        chunks.push(ProviderResponseChunk::Content(ProviderContentData::Text(
            text.to_string(),
        )));
    }
    chunks.push(ProviderResponseChunk::Usage(TokenUsageData {
        input_tokens: Some(input_tokens),
        output_tokens: Some(output_tokens),
        ..Default::default()
    }));
    chunks.push(ProviderResponseChunk::Stop(ResponseStopReason::EndTurn));
    stream_from_chunks(chunks)
}

/// 空文本响应（仅 Stop 帧）——用于空响应重试路径。
pub fn empty_text_stream() -> ProviderResponseStream {
    text_stream("")
}

/// 错误响应（单 Error 终止帧）。
pub fn error_stream(error: ProviderError) -> ProviderResponseStream {
    stream_from_chunks(vec![ProviderResponseChunk::Error(error)])
}

/// 由显式片段列表构造流（需精确控制帧序时使用）。
pub fn stream_from_chunks(chunks: Vec<ProviderResponseChunk>) -> ProviderResponseStream {
    Box::pin(futures_util::stream::iter(chunks))
}
