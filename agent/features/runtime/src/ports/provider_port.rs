//! ProviderPort — Provider BC 出站端口。
//!
//! 对应设计：
//! - `docs/design/02-modules/runtime/06-ports-and-adapters.md` §2
//! - `docs/design/02-modules/provider/02-ports-stream-and-client-scope.md`
//!
//! #901 冻结契约：
//! - PL 类型（ModelIdData、ModelCapabilityData、ProviderError、ProviderResponse 等）
//!   由 Provider crate 的 `published_language` 模块定义。
//! - Runtime 定义 `ProviderPort` trait，引用 Provider PL 类型，
//!   **NEVER** 引用 vendor wire DTO。
//! - `invoke` 返回 pull-based 有序流，终结语义由 `ProviderResponseChunk`
//!   （`Stop`/`Error` 终止帧）表达（#1880 v3 契约）。

use async_trait::async_trait;

// Provider PL 类型 re-export —— 消费方只需 `use crate::ports::provider_port::*`。
// 通过 provider:: 根导出访问，不直接引用 published_language 模块。
// 新 PL StopReason 通过别名 ProviderStopReasonData 导出，此处还原为 StopReason。
pub use provider::{
    InvocationRequestData, ModelCapabilityData, ModelIdData, ProviderError, ProviderResponseStream,
    ProviderStopReasonData as StopReason, RequestSystemBlockData, TokenUsageData,
};

#[cfg(test)]
pub use provider::{
    ProviderContentData, ProviderErrorKind, ProviderResponseChunk, ReasoningCapabilityData,
};

// ReasoningLevel 已由 provider crate 从 core::provider re-export。
pub use share::reasoning::ReasoningLevel;

/// 取消信号（真相源在 runtime Run 生命周期；provider 只消费意图）。
///
/// 端口不暴露取消发起、child token 或 deadline；consumer drop 由私有
/// stream owner 负责转为 invocation-local 取消。
#[async_trait::async_trait]
pub trait CancellationSignal: Send + Sync {
    fn is_cancelled(&self) -> bool;
    async fn cancelled(&self);
}

#[async_trait::async_trait]
impl CancellationSignal for tokio_util::sync::CancellationToken {
    fn is_cancelled(&self) -> bool {
        tokio_util::sync::CancellationToken::is_cancelled(self)
    }

    async fn cancelled(&self) {
        tokio_util::sync::CancellationToken::cancelled(self).await;
    }
}

// ─── Port trait ───

/// Provider BC 的出站端口（内部 ACL）。
///
/// Main/Sub 共享只读 transport；每次 invoke 创建独立 Invocation Scope，
/// 隔离 model/reasoning/max tokens。
///
/// 一次 invoke 最多执行一次上游语义请求。
/// 跨调用 retry、compact、fallback 由 Runtime 负责。
#[async_trait]
pub trait ProviderPort: Send + Sync {
    /// 查询模型能力。
    fn capabilities(&self, model: &ModelIdData) -> Result<ModelCapabilityData, ProviderError>;

    /// 发起一次 LLM 调用，返回单次 attempt 的有序片段流。
    ///
    /// 取消通过 `CancellationToken` 传播；取消后返回 `ProviderError::cancelled()`。
    async fn invoke(
        &self,
        request: InvocationRequestData,
        cancellation: &dyn CancellationSignal,
    ) -> Result<ProviderResponseStream, ProviderError>;
}

// ─── Fake / Contract harness ───────────────────────────

#[cfg(test)]
pub(crate) mod fake {
    //! FakeProvider —— 契约 harness，验证 ProviderPort PL 语义。
    //!
    //! 不依赖真实 HTTP；用于 Runtime 各模块单元测试。

    use super::*;
    use futures::stream;
    use std::sync::Arc;
    use tokio_util::sync::CancellationToken;

    /// 可编程的 fake provider：按预设事件列表依次产出。
    pub struct FakeProvider {
        capabilities: ModelCapabilityData,
    }

    impl FakeProvider {
        /// 构造一个默认 fake provider（supports_tools=true, streaming=true）。
        pub fn new() -> Self {
            Self {
                capabilities: ModelCapabilityData {
                    model: ModelIdData {
                        provider: "fake".to_string(),
                        model: "test-model".to_string(),
                    },
                    supports_tools: true,
                    supports_parallel_tool_calls: true,
                    supports_streaming: true,
                    reasoning: ReasoningCapabilityData::none(),
                    context_limit: Some(128_000),
                    output_limit: Some(8_192),
                },
            }
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

    #[async_trait]
    impl ProviderPort for FakeProvider {
        fn capabilities(&self, model: &ModelIdData) -> Result<ModelCapabilityData, ProviderError> {
            if model.provider == "fake" {
                Ok(self.capabilities.clone())
            } else {
                Err(ProviderError::fatal(
                    ProviderErrorKind::ModelUnavailable,
                    format!("unknown model: {model}"),
                ))
            }
        }

        async fn invoke(
            &self,
            _request: InvocationRequestData,
            cancellation: &dyn CancellationSignal,
        ) -> Result<ProviderResponseStream, ProviderError> {
            if cancellation.is_cancelled() {
                return Err(ProviderError::cancelled());
            }
            Ok(Self::happy_path_stream("hello"))
        }
    }

    pub struct FakeProviderFactory;

    impl crate::ports::ProviderFactory for FakeProviderFactory {
        fn build(
            &self,
            spec: crate::ports::ProviderBuildSpecData,
        ) -> Result<crate::ports::ProviderBindingData, ProviderError> {
            Ok(crate::ports::ProviderBindingData {
                provider: Arc::new(FakeProvider::new()),
                model: spec.model,
                max_tokens: spec.max_tokens,
                requested_reasoning: spec.requested_reasoning,
                context_window: spec.context_window,
            })
        }
    }

    // ─── 契约测试 ───

    #[test]
    fn fake_provider_capabilities_returns_for_matching_model() {
        let provider = FakeProvider::new();
        let model = ModelIdData {
            provider: "fake".to_string(),
            model: "test-model".to_string(),
        };
        let cap = provider.capabilities(&model).unwrap();
        assert!(cap.supports_tools);
        assert!(cap.supports_streaming);
        assert_eq!(cap.context_limit, Some(128_000));
    }

    #[test]
    fn fake_provider_capabilities_rejects_unknown_model() {
        let provider = FakeProvider::new();
        let model = ModelIdData {
            provider: "unknown".to_string(),
            model: "x".to_string(),
        };
        let err = provider.capabilities(&model).unwrap_err();
        assert_eq!(err.kind, ProviderErrorKind::ModelUnavailable);
        assert!(!err.retryable);
    }

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

    #[tokio::test]
    async fn invoke_returns_cancelled_error_when_already_cancelled() {
        let provider = FakeProvider::new();
        let cancel = CancellationToken::new();
        cancel.cancel();

        let request = InvocationRequestData::new(
            ModelIdData {
                provider: "fake".to_string(),
                model: "test-model".to_string(),
            },
            Vec::new(),
            8192,
            ReasoningLevel::Off,
        );

        let result = provider.invoke(request, &cancel).await;
        match result {
            Err(e) => assert!(e.is_cancelled()),
            Ok(_) => panic!("expected cancelled error"),
        }
    }

    #[tokio::test]
    async fn invoke_returns_stream_with_correct_terminal_semantics() {
        let provider = FakeProvider::new();
        let cancel = CancellationToken::new();
        let request = InvocationRequestData::new(
            ModelIdData {
                provider: "fake".to_string(),
                model: "test-model".to_string(),
            },
            Vec::new(),
            8192,
            ReasoningLevel::Off,
        );

        let mut stream = provider.invoke(request, &cancel).await.unwrap();
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
    async fn provider_port_accepts_object_safe_cancellation_signal() {
        struct AlwaysCancelled;

        #[async_trait]
        impl CancellationSignal for AlwaysCancelled {
            fn is_cancelled(&self) -> bool {
                true
            }

            async fn cancelled(&self) {}
        }

        let provider = FakeProvider::new();
        let request = InvocationRequestData::new(
            ModelIdData {
                provider: "fake".to_string(),
                model: "test-model".to_string(),
            },
            Vec::new(),
            8192,
            ReasoningLevel::Off,
        );

        let result = provider.invoke(request, &AlwaysCancelled).await;
        assert!(matches!(result, Err(error) if error.is_cancelled()));
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
