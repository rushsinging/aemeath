//! ProviderPort — Provider BC 出站端口。
//!
//! 对应设计：`docs/design/02-modules/runtime/06-ports-and-adapters.md` §2。
//! PL 类型细化由 #901 负责；此处只定义最小骨架。

use async_trait::async_trait;
use futures::Stream;
use tokio_util::sync::CancellationToken;

// ─── Published Language（最小骨架，#901 迁移到 provider crate） ───

/// 模型标识符。
// TODO(#901): 迁移到 provider crate。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ModelId {
    pub provider: String,
    pub model: String,
}

impl std::fmt::Display for ModelId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.provider, self.model)
    }
}

/// 模型能力声明。
// TODO(#901): 迁移到 provider crate 并细化字段。
#[derive(Debug, Clone)]
pub struct ModelCapability {
    /// 模型最大 reasoning level。
    pub max_reasoning: provider::api::ReasoningLevel,
    /// 模型上下文窗口大小（token 数）。
    pub context_window: usize,
    /// 最大输出 token 数。
    pub max_output_tokens: usize,
}

/// Provider 结构化错误分类。
///
/// Runtime 拥有 retry/compact/fallback 策略；Provider 只负责分类错误。
#[derive(Debug, Clone, thiserror::Error)]
pub enum ProviderError {
    /// 可重试错误（超时 / 5xx / 429 / 流中断）。
    #[error("retryable provider error: {0}")]
    Retryable(String),

    /// 不可重试错误（4xx / 参数无效 / 模型不存在）。
    #[error("fatal provider error: {0}")]
    Fatal(String),

    /// 上下文超限——Runtime 应触发 compact。
    #[error("context length exceeded")]
    ContextExceeded,
}

/// 一次 LLM 调用请求。
// TODO(#901): 迁移到 provider crate 并细化字段。
#[derive(Debug, Clone)]
pub struct InvocationRequest {
    /// 目标模型。
    pub model: ModelId,
    /// 本轮上下文窗口消息。
    pub messages: Vec<share::message::Message>,
    /// 期望 reasoning level。
    pub reasoning: provider::api::ReasoningLevel,
}

/// 一次 LLM 调用的有序流式响应。
///
/// Provider 返回单次 attempt 的有序 delta 流；Runtime 负责流汇聚和 tool_call 提取。
// TODO(#901): 迁移到 provider crate 并细化 delta 类型。
pub type InvocationStream =
    std::pin::Pin<Box<dyn Stream<Item = provider::api::DeltaPayload> + Send>>;

// ─── Port trait ───

/// Provider BC 的出站端口（内部 ACL）。
///
/// Main/Sub 共享只读 transport；每次 invoke 创建独立 Invocation Scope，
/// 隔离 model/reasoning/max tokens。
#[async_trait]
pub trait ProviderPort: Send + Sync {
    /// 查询模型能力。
    fn capabilities(&self, model: &ModelId) -> Result<ModelCapability, ProviderError>;

    /// 发起一次 LLM 调用，返回单次 attempt 的有序流。
    async fn invoke(
        &self,
        request: InvocationRequest,
        cancellation: &CancellationToken,
    ) -> Result<InvocationStream, ProviderError>;
}
