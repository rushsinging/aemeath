//! Provider 出站能力（#2146 收敛后）。  
//!
//! 对应设计：
//! - `docs/design/02-modules/runtime/06-ports-and-adapters.md` §2
//! - `docs/design/02-modules/provider/02-ports-stream-and-client-scope.md`
//!
//! #2146 契约变更（收敛 #901 的 runtime 自有 `ProviderPort` + #1863 的
//! provider `LlmTextInvokePort`）：
//! - LLM 调用能力端口 `LlmInvokePort` **由 provider crate 唯一定义**：
//!   流式 `invoke_stream` 是唯一必须实现的 verb，文本 `invoke_text` 是
//!   端口默认方法（非流式即流式收口，不再作为独立 trait）。
//! - 取消真相源单通道：`ProviderRequestData.cancellation`
//!   （runtime 不再经独立 `CancellationSignal` 参数传取消）。
//! - 本模块只保留 PL re-export 与契约 harness；runtime 内部持有
//!   `Arc<dyn LlmInvokePort>`（`ProviderBindingData.provider`）。
//!
//! #901 冻结的 PL 类型（ModelInfo、ProviderError、ProviderResponse 等）
//! 仍由 provider crate 的 `published_language` 定义；Runtime **NEVER**
//! 引用 vendor wire DTO。

// Provider PL 类型 re-export —— 消费方只需 `use crate::ports::provider_port::*`。
// 通过 provider:: 根导出访问，不直接引用 published_language 模块。
// 新 PL StopReason 通过别名 ResponseStopReason 导出，此处还原为 StopReason。
pub use provider::{
    LlmInvokePort, ModelInfo, ProviderError, ProviderRequestData, ResponseStopReason as StopReason,
    TokenUsageData,
};

#[cfg(test)]
pub use provider::{
    ProviderContentData, ProviderErrorKind, ProviderResponseChunk, ProviderResponseStream,
};

// ReasoningLevel 已由 provider crate 从 core::provider re-export。
pub use share::reasoning::ReasoningLevel;

// ─── Fake / Contract harness ───────────────────────────

#[cfg(test)]
pub(crate) mod fake {
    //! FakeProvider —— 契约 harness，验证 LLM 调用端口的 PL 语义。
    //!
    //! 不依赖真实 HTTP；用于 Runtime 各模块单元测试。

    use super::*;
    use futures::stream;
    use std::sync::Arc;

    /// 可编程的 fake provider：按预设事件列表依次产出。
    pub struct FakeProvider;

    impl FakeProvider {
        /// 构造一个默认 fake provider。
        pub fn new() -> Self {
            Self
        }

        /// 生成一个产出 `chunks` 后终结的 ProviderResponseStream。
        pub fn stream_from(chunks: Vec<ProviderResponseChunk>) -> ProviderResponseStream {
            Box::pin(stream::iter(chunks))
        }

        /// 生成一个文本 Content 帧 + Usage/Stop 尾帧的正常流。
        pub fn happy_path_stream(text: &str) -> ProviderResponseStream {
            let chunks = vec![
                ProviderResponseChunk::Content(ProviderContentData::Text(text.to_string())),
                ProviderResponseChunk::Usage(TokenUsageData {
                    input_tokens: Some(10),
                    output_tokens: Some(5),
                    ..Default::default()
                }),
                ProviderResponseChunk::Stop(StopReason::EndTurn),
            ];
            Self::stream_from(chunks)
        }

        /// 生成一个直接失败的流（Error 终止帧）。
        pub fn error_stream(error: ProviderError) -> ProviderResponseStream {
            Self::stream_from(vec![ProviderResponseChunk::Error(error)])
        }
    }

    impl Default for FakeProvider {
        fn default() -> Self {
            Self::new()
        }
    }

    /// FakeProvider 的调用端口（#2146：实现归 provider test_support，
    /// 契约 harness 只提供脚本与流构造）。
    pub(crate) fn port() -> Arc<dyn LlmInvokePort> {
        provider::test_support::scripted_llm(|request| {
            // 契约 harness 的取消语义由脚本显式表达（回放器不拦截）。
            if request.cancellation.is_cancelled() {
                return Err(ProviderError::cancelled());
            }
            Ok(FakeProvider::happy_path_stream("hello"))
        })
    }

    pub struct FakeProviderFactory;

    impl crate::ports::ProviderFactory for FakeProviderFactory {
        fn build(
            &self,
            spec: crate::ports::ProviderBuildSpecData,
        ) -> Result<crate::ports::ProviderBindingData, ProviderError> {
            Ok(crate::ports::ProviderBindingData {
                provider: port(),
                model: crate::ports::ModelInfo {
                    provider: spec.source_key.clone(),
                    model: spec.model.clone(),
                    supports_tools: true,
                    supports_parallel_tool_calls: true,
                    supports_streaming: true,
                    supported_reasoning: vec![share::reasoning::ReasoningLevel::Off],
                    server_effort_map: Vec::new(),
                    context_limit: spec.context_window,
                    output_limit: Some(spec.max_tokens as usize),
                },
                max_tokens: spec.max_tokens,
                requested_reasoning: spec.requested_reasoning,
            })
        }
    }

    // ─── 契约测试 ───
    //
    // `fake_provider_capabilities_*` 两个查询契约测试已删除（#1880）：
    // 端口不再暴露 capabilities()——unknown model 门禁语义移到装配时
    // （factory build 构造 ModelInfo），运行时零查询。
    // `provider_port_accepts_object_safe_cancellation_signal` 已删除（#2146）：
    // 端口不再有独立取消参数，取消单通道走 `request.cancellation`。

    #[tokio::test]
    async fn happy_path_stream_emits_content_then_usage_then_stop() {
        let stream = FakeProvider::happy_path_stream("hi");
        futures::pin_mut!(stream);
        use futures::StreamExt;

        let first = stream.next().await.unwrap();
        assert!(matches!(
            first,
            ProviderResponseChunk::Content(ProviderContentData::Text(ref t)) if t == "hi"
        ));

        let second = stream.next().await.unwrap();
        match second {
            ProviderResponseChunk::Usage(usage) => {
                assert_eq!(usage.input_tokens, Some(10));
            }
            other => panic!("expected Usage frame, got {other:?}"),
        }

        let third = stream.next().await.unwrap();
        assert!(
            matches!(third, ProviderResponseChunk::Stop(StopReason::EndTurn)),
            "expected Stop frame, got {third:?}"
        );

        // 终结后 next() 返回 None
        assert!(stream.next().await.is_none());
    }

    #[tokio::test]
    async fn error_stream_emits_error_then_none() {
        let stream = FakeProvider::error_stream(ProviderError::cancelled());
        futures::pin_mut!(stream);
        use futures::StreamExt;

        let first = stream.next().await.unwrap();
        assert!(matches!(first, ProviderResponseChunk::Error(_)));

        // 终结后 next() 返回 None
        assert!(stream.next().await.is_none());
    }

    fn model() -> ModelInfo {
        ModelInfo {
            provider: "fake".to_string(),
            model: "test-model".to_string(),
            supports_tools: false,
            supports_parallel_tool_calls: false,
            supports_streaming: true,
            supported_reasoning: vec![ReasoningLevel::Off],
            server_effort_map: Vec::new(),
            context_limit: Some(128_000),
            output_limit: Some(8_192),
        }
    }

    #[tokio::test]
    async fn invoke_stream_returns_cancelled_error_when_request_signal_fired() {
        let provider = port();
        let request = ProviderRequestData::new(
            "test-model".to_string(),
            Vec::new(),
            8192,
            ReasoningLevel::Off,
        );
        request.cancellation.cancel();

        let result = provider.invoke_stream(&model(), &request).await;
        match result {
            Err(e) => assert!(e.is_cancelled()),
            Ok(_) => panic!("expected cancelled error"),
        }
    }

    #[tokio::test]
    async fn invoke_stream_returns_stream_with_correct_terminal_semantics() {
        let provider = port();
        let request = ProviderRequestData::new(
            "test-model".to_string(),
            Vec::new(),
            8192,
            ReasoningLevel::Off,
        );

        let mut stream = provider.invoke_stream(&model(), &request).await.unwrap();
        use futures::StreamExt;

        // 收集所有片段
        let mut chunks = Vec::new();
        while let Some(chunk) = stream.next().await {
            chunks.push(chunk);
        }

        // 恰好 3 帧：1 Content + 1 Usage + 1 Stop 终止帧
        assert_eq!(chunks.len(), 3);
        assert!(matches!(chunks[0], ProviderResponseChunk::Content(_)));
        assert!(matches!(chunks[1], ProviderResponseChunk::Usage(_)));
        assert!(matches!(chunks[2], ProviderResponseChunk::Stop(_)));
        assert!(chunks[2].is_terminal());
    }

    #[tokio::test]
    async fn invoke_text_default_method_collects_stream_into_text_output() {
        // #2146：文本是端口默认方法（收口）——任何 invoke_stream 实现免费获得。
        let provider = port();
        let request = ProviderRequestData::new(
            "test-model".to_string(),
            Vec::new(),
            8192,
            ReasoningLevel::Off,
        );

        let output = provider.invoke_text(&model(), &request).await.unwrap();
        assert_eq!(output.text, "hello");
        assert_eq!(output.completion_reason.as_deref(), Some("end_turn"));
        assert_eq!(output.text_delta_count, 1);
        assert!(output.stream_completed);
    }

    #[test]
    fn response_chunk_is_the_provider_published_language_type() {
        fn accepts_provider_chunk(_: provider::ProviderResponseChunk) {}
        accepts_provider_chunk(ProviderResponseChunk::Error(ProviderError::cancelled()));
    }

    #[test]
    fn provider_error_kinds_are_distinct() {
        let cancelled = ProviderError::cancelled();
        let context = ProviderError::fatal(ProviderErrorKind::ContextTooLong, "too long");
        let rate = ProviderError::retryable(ProviderErrorKind::RateLimited, "429");

        assert_eq!(cancelled.kind, ProviderErrorKind::Cancelled);
        assert_eq!(context.kind, ProviderErrorKind::ContextTooLong);
        assert_eq!(rate.kind, ProviderErrorKind::RateLimited);

        assert!(!cancelled.retryable);
        assert!(!context.retryable);
        assert!(rate.retryable);
    }
}
