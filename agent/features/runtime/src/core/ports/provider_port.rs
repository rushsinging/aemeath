//! ProviderPort — Provider BC 出站端口。
//!
//! 对应设计：
//! - `docs/design/02-modules/runtime/06-ports-and-adapters.md` §2
//! - `docs/design/02-modules/provider/02-ports-stream-and-client-scope.md`
//!
//! #901 冻结契约：
//! - PL 类型（ModelId、ModelCapability、ProviderError、ProviderCompletion 等）
//!   由 Provider crate 的 `published_language` 模块定义。
//! - Runtime 定义 `ProviderPort` trait、`InvocationStream` 和 `InvocationEvent`，
//!   引用 Provider PL 类型，**NEVER** 引用 vendor wire DTO。
//! - `invoke` 返回 pull-based 有序流，终结语义由 `InvocationEvent` 表达。

use std::pin::Pin;

use async_trait::async_trait;
use futures::Stream;
use tokio_util::sync::CancellationToken;

// Provider PL 类型 re-export —— 消费方只需 `use crate::core::ports::provider_port::*`。
// 通过 provider::api（API facade）访问，不直接引用 published_language 模块。
// 新 PL StopReason 通过别名 ProviderStopReason 导出，此处还原为 StopReason。
pub use provider::api::{
    InvocationDelta, InvocationOptions, InvocationRequest, ModelCapability, ModelId,
    ModelToolSchema, ProviderCompletion, ProviderContentBlock, ProviderError, ProviderErrorKind,
    ProviderStopReason as StopReason, ProviderToolCall, ProviderToolCallId, RawUsageSnapshot,
    ReasoningCapability, ReasoningMappingKind,
};

// ReasoningLevel 已由 provider crate 从 core::provider re-export。
pub use provider::api::ReasoningLevel;

// ─── InvocationEvent / InvocationStream ────────────────

/// 一次调用的流式事件。
///
/// 终结语义：
/// - `Delta` 只表达非终结增量；
/// - `Completed` 与 `Failed` 互斥，恰好出现一个；
/// - 取消以 `Failed(ProviderError::cancelled())` 终结；
/// - 终结事件后下一次 `next()` 返回 `None`。
#[derive(Debug, Clone)]
pub enum InvocationEvent {
    /// 非终结增量。
    Delta(InvocationDelta),
    /// 完成终结。
    Completed(ProviderCompletion),
    /// 失败终结（含取消）。
    Failed(ProviderError),
}

/// 一次 LLM 调用的有序 pull-based 流。
///
/// Provider 返回单次 attempt 的有序事件流；Runtime 负责流汇聚、
/// tool_call 提取和领域事件投影。
///
/// consumer drop 等价于取消意图，adapter 应停止继续读取和缓冲。
pub type InvocationStream = Pin<Box<dyn Stream<Item = InvocationEvent> + Send>>;

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
    fn capabilities(&self, model: &ModelId) -> Result<ModelCapability, ProviderError>;

    /// 发起一次 LLM 调用，返回单次 attempt 的有序流。
    ///
    /// 取消通过 `CancellationToken` 传播；取消后返回 `ProviderError::cancelled()`。
    async fn invoke(
        &self,
        request: InvocationRequest,
        cancellation: &CancellationToken,
    ) -> Result<InvocationStream, ProviderError>;
}

// ─── Fake / Contract harness ───────────────────────────

#[cfg(test)]
pub(crate) mod fake {
    //! FakeProvider —— 契约 harness，验证 ProviderPort PL 语义。
    //!
    //! 不依赖真实 HTTP；用于 Runtime 各模块单元测试。

    use super::*;
    use futures::stream;

    /// 可编程的 fake provider：按预设事件列表依次产出。
    pub struct FakeProvider {
        capabilities: ModelCapability,
    }

    impl FakeProvider {
        /// 构造一个默认 fake provider（supports_tools=true, streaming=true）。
        pub fn new() -> Self {
            Self {
                capabilities: ModelCapability {
                    model: ModelId {
                        provider: "fake".to_string(),
                        model: "test-model".to_string(),
                    },
                    supports_tools: true,
                    supports_parallel_tool_calls: true,
                    supports_streaming: true,
                    reasoning: ReasoningCapability::none(),
                    context_limit: Some(128_000),
                    output_limit: Some(8_192),
                },
            }
        }

        /// 生成一个产出 `events` 后终结的 InvocationStream。
        pub fn stream_from(events: Vec<InvocationEvent>) -> InvocationStream {
            Box::pin(stream::iter(events))
        }

        /// 生成一个文本 delta + Completed 终结的正常流。
        pub fn happy_path_stream(text: &str) -> InvocationStream {
            let events = vec![
                InvocationEvent::Delta(InvocationDelta::Text(text.to_string())),
                InvocationEvent::Completed(ProviderCompletion {
                    output: vec![ProviderContentBlock::Text(text.to_string())],
                    stop_reason: StopReason::EndTurn,
                    usage: Some(RawUsageSnapshot {
                        input_tokens: Some(10),
                        output_tokens: Some(5),
                        ..Default::default()
                    }),
                    effective_reasoning: ReasoningLevel::Off,
                }),
            ];
            Self::stream_from(events)
        }

        /// 生成一个直接失败的流。
        pub fn error_stream(error: ProviderError) -> InvocationStream {
            Self::stream_from(vec![InvocationEvent::Failed(error)])
        }
    }

    impl Default for FakeProvider {
        fn default() -> Self {
            Self::new()
        }
    }

    #[async_trait]
    impl ProviderPort for FakeProvider {
        fn capabilities(&self, model: &ModelId) -> Result<ModelCapability, ProviderError> {
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
            _request: InvocationRequest,
            cancellation: &CancellationToken,
        ) -> Result<InvocationStream, ProviderError> {
            if cancellation.is_cancelled() {
                return Err(ProviderError::cancelled());
            }
            Ok(Self::happy_path_stream("hello"))
        }
    }

    // ─── 契约测试 ───

    #[test]
    fn fake_provider_capabilities_returns_for_matching_model() {
        let provider = FakeProvider::new();
        let model = ModelId {
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
        let model = ModelId {
            provider: "unknown".to_string(),
            model: "x".to_string(),
        };
        let err = provider.capabilities(&model).unwrap_err();
        assert_eq!(err.kind, ProviderErrorKind::ModelUnavailable);
        assert!(!err.retryable);
    }

    #[tokio::test]
    async fn happy_path_stream_emits_delta_then_completed() {
        let stream = FakeProvider::happy_path_stream("hi");
        futures::pin_mut!(stream);
        use futures::StreamExt;

        let first = stream.next().await.unwrap();
        assert!(matches!(
            first,
            InvocationEvent::Delta(InvocationDelta::Text(ref t)) if t == "hi"
        ));

        let second = stream.next().await.unwrap();
        match second {
            InvocationEvent::Completed(c) => {
                assert_eq!(c.stop_reason, StopReason::EndTurn);
                assert_eq!(c.usage.unwrap().input_tokens, Some(10));
            }
            _ => panic!("expected Completed"),
        }

        // 终结后 next() 返回 None
        assert!(stream.next().await.is_none());
    }

    #[tokio::test]
    async fn error_stream_emits_failed_then_none() {
        let stream = FakeProvider::error_stream(ProviderError::cancelled());
        futures::pin_mut!(stream);
        use futures::StreamExt;

        let first = stream.next().await.unwrap();
        assert!(matches!(first, InvocationEvent::Failed(_)));

        // 终结后 next() 返回 None
        assert!(stream.next().await.is_none());
    }

    #[tokio::test]
    async fn invoke_returns_cancelled_error_when_already_cancelled() {
        let provider = FakeProvider::new();
        let cancel = CancellationToken::new();
        cancel.cancel();

        let request = InvocationRequest::new(
            ModelId {
                provider: "fake".to_string(),
                model: "test-model".to_string(),
            },
            Vec::new(),
            InvocationOptions::new(8192, ReasoningLevel::Off),
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
        let request = InvocationRequest::new(
            ModelId {
                provider: "fake".to_string(),
                model: "test-model".to_string(),
            },
            Vec::new(),
            InvocationOptions::new(8192, ReasoningLevel::Off),
        );

        let mut stream = provider.invoke(request, &cancel).await.unwrap();
        use futures::StreamExt;

        // 收集所有事件
        let mut events = Vec::new();
        while let Some(evt) = stream.next().await {
            events.push(evt);
        }

        // 恰好 2 个事件：1 个 Delta + 1 个 Completed
        assert_eq!(events.len(), 2);
        assert!(matches!(events[0], InvocationEvent::Delta(_)));
        assert!(matches!(events[1], InvocationEvent::Completed(_)));
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
