//! Provider Published Language — 跨 BC 中立契约。
//!
//! 对应设计：
//! - `docs/design/02-modules/provider/01-domain-model-and-acl.md`
//! - `docs/design/02-modules/provider/02-ports-stream-and-client-scope.md`
//!
//! 这些类型是 Provider 对外发布的稳定语义边界。
//! Runtime 只通过这些类型消费 Provider，**NEVER** 直接引用 vendor wire DTO。
//!
//! #901 冻结契约；现有 `contract.rs` 的 legacy 类型保留兼容，后续逐步退役。

use share::reasoning::ReasoningLevel;
use std::time::Duration;

use share::message::Message;

// ─── 模型标识 ───────────────────────────────────────────

/// 模型标识符（provider/model）。
///
/// 跨 BC 稳定标识一个 LLM 模型源，不携带 driver 或 transport 细节。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ModelIdData {
    /// provider 名称（如 "Anthropic"、"Zhipu"）。
    pub provider: String,
    /// 模型名称（如 "claude-fable-5-1"）。
    pub model: String,
}

impl std::fmt::Display for ModelIdData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.provider, self.model)
    }
}

// ─── Reasoning ──────────────────────────────────────────

/// 模型 reasoning 能力声明。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ReasoningCapabilityData {
    supported: Vec<ReasoningLevel>,
}

impl ReasoningCapabilityData {
    pub fn new(supported: impl IntoIterator<Item = ReasoningLevel>) -> Result<Self, ProviderError> {
        let mut supported: Vec<_> = supported.into_iter().collect();
        supported.sort_unstable();
        supported.dedup();
        if supported.first() != Some(&ReasoningLevel::Off) {
            return Err(ProviderError::fatal(
                ProviderErrorKind::Configuration,
                "reasoning capability 必须包含 off 档位",
            ));
        }
        Ok(Self { supported })
    }

    /// 构造不支持 reasoning 的默认能力。
    pub fn none() -> Self {
        Self {
            supported: vec![ReasoningLevel::Off],
        }
    }

    pub fn supported(&self) -> &[ReasoningLevel] {
        &self.supported
    }

    pub fn maximum(&self) -> ReasoningLevel {
        self.supported
            .last()
            .copied()
            .unwrap_or(ReasoningLevel::Off)
    }

    pub fn resolve(&self, requested: ReasoningLevel) -> ReasoningLevel {
        self.supported
            .iter()
            .rev()
            .copied()
            .find(|level| *level <= requested)
            .unwrap_or(ReasoningLevel::Off)
    }
}

// ─── ModelCapabilityData ────────────────────────────────────

/// 模型能力声明。
///
/// Runtime 可用于前置校验和展示；Provider 在请求编码前仍必须复核。
#[derive(Debug, Clone)]
pub struct ModelCapabilityData {
    /// 模型标识。
    pub model: ModelIdData,
    /// 是否支持 tool use。
    pub supports_tools: bool,
    /// 是否支持并行 tool calls。
    pub supports_parallel_tool_calls: bool,
    /// 是否支持流式。
    pub supports_streaming: bool,
    /// Reasoning 能力。
    pub reasoning: ReasoningCapabilityData,
    /// 上下文窗口大小（token 数），`None` 表示未知。
    pub context_limit: Option<usize>,
    /// 最大输出 token 数，`None` 表示未知。
    pub output_limit: Option<usize>,
}

impl ModelCapabilityData {}

// ─── Tool Call（Provider 边界） ─────────────────────────

/// Provider 边界的 tool call 完整形态。
///
/// ID 为 Provider 返回的原始 tool-call 标识（如 Anthropic 的 `toolu_*` 或
/// OpenAI 的 `call_*`）。Runtime 在写入 Run Step 时创建领域 `ToolCallId`
/// 并维护双 ID 映射。Provider **NEVER** 生成领域 ID。
#[derive(Debug, Clone)]
pub struct ProviderToolCallData {
    /// Provider 原始 tool-call ID。
    pub id: String,
    /// 工具名称。
    pub name: String,
    /// 验证过的 JSON 参数。
    pub arguments: serde_json::Value,
}

// ─── Raw Usage ──────────────────────────────────────────

/// 原始 token 使用快照。
///
/// 所有字段区分"未报告"（`None`）与真实零值（`Some(0)`）。
/// Provider 只做协议标准化，不计算 cost。
#[derive(Debug, Clone, Default)]
pub struct TokenUsageData {
    pub input_tokens: Option<u32>,
    pub output_tokens: Option<u32>,
    pub cache_read_tokens: Option<u32>,
    pub cache_write_tokens: Option<u32>,
    pub reasoning_tokens: Option<u32>,
}

impl TokenUsageData {
    pub fn was_reported(&self) -> bool {
        self.input_tokens.is_some()
            || self.output_tokens.is_some()
            || self.cache_read_tokens.is_some()
            || self.cache_write_tokens.is_some()
            || self.reasoning_tokens.is_some()
    }

    pub fn into_reported(self) -> Option<Self> {
        self.was_reported().then_some(self)
    }

    pub fn merge_reported(&mut self, latest: Self) {
        if latest.input_tokens.is_some() {
            self.input_tokens = latest.input_tokens;
        }
        if latest.output_tokens.is_some() {
            self.output_tokens = latest.output_tokens;
        }
        if latest.cache_read_tokens.is_some() {
            self.cache_read_tokens = latest.cache_read_tokens;
        }
        if latest.cache_write_tokens.is_some() {
            self.cache_write_tokens = latest.cache_write_tokens;
        }
        if latest.reasoning_tokens.is_some() {
            self.reasoning_tokens = latest.reasoning_tokens;
        }
    }
}

// ─── Stop Reason ────────────────────────────────────────

/// 统一停止原因。
///
/// 注意：与 legacy `business::types::StopReason`（3 变体）不同。
/// 对外 re-export 时使用别名 `ProviderStopReasonData` 以避免命名冲突。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StopReason {
    /// 模型自然结束回复。
    EndTurn,
    /// 模型请求执行 tool。
    ToolUse,
    /// 达到最大输出 token。
    MaxOutputTokens,
    /// 内容被安全过滤。
    ContentFiltered,
    /// 命中 stop sequence。
    StopSequence,
    /// 其他原因，保留 provider 原始 code 供诊断。
    Other(String),
}

/// 别名导出——contract.rs 用此名 re-export，避免与 legacy StopReason 冲突。
pub use StopReason as ProviderStopReasonData;

// ─── Response（输出侧唯一契约：非流式终态 + 流式片段）──────────────

/// 一次调用的完整响应（非流式终态；自带成败——单通道形态）。
///
/// `ok=false` 时仅 `error` 有意义；流式调用的消费方聚合片段后组装同构
/// 实例（Usage/Stop 片段归位于此）。
#[derive(Debug, Clone)]
pub struct ProviderResponse {
    pub ok: bool,
    pub error: Option<ProviderError>,
    pub output: Vec<ProviderContentData>,
    pub stop_reason: Option<ProviderStopReasonData>,
    pub token_usage: Option<TokenUsageData>,
    pub effective_reasoning: ReasoningLevel,
}

/// 流式响应的增量片段；`Error` 为失败终止帧（取消同归于此）。
#[derive(Debug, Clone)]
pub enum ProviderResponseChunk {
    /// 内容片段（文本/thinking/工具调用增量与完整块）。
    Content(ProviderContentData),
    /// LLM token 用量（流尾）。
    Usage(TokenUsageData),
    /// 停止原因（流尾，正常闭合）。
    Stop(ProviderStopReasonData),
    /// 失败终止帧。
    Error(ProviderError),
}

/// 内容片段——「终态块」与「流增量」合并的同构家族。
#[derive(Debug, Clone)]
pub enum ProviderContentData {
    /// 文本（增量或完整块）。
    Text(String),
    /// Thinking/reasoning 内容（签名可选）。
    Thinking {
        thinking: String,
        signature: Option<String>,
    },
    /// 完整 tool call（终态块 / 增量完成帧）。
    ToolCall(ProviderToolCallData),
    /// Tool call 开始（流增量）。
    ToolCallStarted {
        index: usize,
        provider_id: Option<String>,
        name: String,
    },
    /// Tool arguments 增量字符串片段（流增量）。
    ToolArgumentsDelta {
        index: usize,
        provider_id: Option<String>,
        partial_json: String,
    },
    /// Tool call 增量完成帧：参数已完整并校验为合法 JSON（`index` 与
    /// `ToolCallStarted`/`ToolArgumentsDelta` 同源），与终态 `ToolCall`
    /// 块同构——runtime 据此边流边执行。
    ToolCallCompleted {
        index: usize,
        call: ProviderToolCallData,
    },
}

/// 流式响应类型。
pub type ProviderResponseStream =
    std::pin::Pin<Box<dyn futures_util::Stream<Item = ProviderResponseChunk> + Send>>;

impl ProviderResponseChunk {
    /// 终止帧：`Stop`（正常闭合）或 `Error`（失败闭合，含取消）；
    /// 终止帧之后流必须结束。
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Stop(_) | Self::Error(_))
    }
}

// ─── Invocation Delta ───────────────────────────────────

// ─── Error ──────────────────────────────────────────────

/// Provider 结构化错误分类。
///
/// Runtime 拥有 retry/compact/fallback 策略；Provider 只负责分类错误并提供提示。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProviderErrorKind {
    /// 调用被取消。
    #[error("cancelled")]
    Cancelled,
    /// 认证失败。
    #[error("authentication failed")]
    Authentication,
    /// 权限被拒绝。
    #[error("permission denied")]
    PermissionDenied,
    /// 速率限制。
    #[error("rate limited")]
    RateLimited,
    /// 上下文超限——Runtime 应触发 compact。
    #[error("context too long")]
    ContextTooLong,
    /// 请求参数无效。
    #[error("invalid request")]
    InvalidRequest,
    /// 模型不可用。
    #[error("model unavailable")]
    ModelUnavailable,
    /// 上游不可用（5xx）。
    #[error("upstream unavailable")]
    UpstreamUnavailable,
    /// 网络错误。
    #[error("network error")]
    Network,
    /// 超时。
    #[error("timeout")]
    Timeout,
    /// 协议错误。
    #[error("protocol error")]
    Protocol,
    /// 流在 tool arguments 中间被截断。
    #[error("stream truncated")]
    StreamTruncated,
    /// 配置错误。
    #[error("configuration error")]
    Configuration,
}

/// Provider 完整错误。
///
/// `retryable` 是 Provider 对失败性质的提示，不是重试命令。
/// `retry_after` 只承载经校验的协议等待 hint（如 `Retry-After` header）。
#[derive(Debug, Clone)]
pub struct ProviderError {
    /// 错误分类。
    pub kind: ProviderErrorKind,
    /// 是否建议重试。
    pub retryable: bool,
    /// 安全的错误描述（已脱敏）。
    pub safe_message: String,
    /// Provider 原始错误码（如 HTTP status code 字符串），用于诊断。
    pub provider_code: Option<String>,
    /// 协议建议的等待时间（如 429 Retry-After）。
    pub retry_after: Option<Duration>,
}

impl ProviderError {
    /// 构造一个不可重试的 fatal 错误。
    pub fn fatal(kind: ProviderErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            retryable: false,
            safe_message: message.into(),
            provider_code: None,
            retry_after: None,
        }
    }

    /// 构造一个可重试错误。
    pub fn retryable(kind: ProviderErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            retryable: true,
            safe_message: message.into(),
            provider_code: None,
            retry_after: None,
        }
    }

    /// 取消错误（不可重试）。
    pub fn cancelled() -> Self {
        Self::fatal(ProviderErrorKind::Cancelled, "request cancelled")
    }

    /// 是否为取消。
    pub fn is_cancelled(&self) -> bool {
        self.kind == ProviderErrorKind::Cancelled
    }

    /// 是否为上下文超限。
    pub fn is_context_exceeded(&self) -> bool {
        self.kind == ProviderErrorKind::ContextTooLong
    }
}

impl std::fmt::Display for ProviderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.kind, self.safe_message)
    }
}

impl std::error::Error for ProviderError {}

// ─── System Blocks (provider-neutral) ──────────────────

/// Provider-neutral system prompt 块。
///
/// Runtime 构造的 system prompt 内容，区分可缓存（静态、稳定）与动态文本。
/// `Cacheable` 表示该块适合 prompt caching；是否真正命中缓存由 provider 决定。
/// driver/adapter 负责转换到 vendor wire DTO（如 Anthropic 的 `SystemBlockData`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RequestSystemBlockData {
    /// 动态文本块，不参与 prompt caching。
    Text(String),
    /// 静态文本块，建议 provider 应用 prompt caching（如 Anthropic ephemeral）。
    Cacheable(String),
}

impl RequestSystemBlockData {
    /// 块的文本内容。
    pub fn text(&self) -> &str {
        match self {
            RequestSystemBlockData::Text(t) | RequestSystemBlockData::Cacheable(t) => t,
        }
    }

    /// 是否建议 provider 缓存。
    pub fn is_cacheable(&self) -> bool {
        matches!(self, RequestSystemBlockData::Cacheable(_))
    }
}

// ─── InvocationRequestData ──────────────────────────────────

/// 一次 LLM 调用请求。
///
/// 一个 `InvocationRequestData` 固定一个 model 和一份不可变 options。
#[derive(Debug, Clone)]
pub struct InvocationRequestData {
    /// 目标模型。
    pub model: ModelIdData,
    /// Runtime-owned cancellation token for this invocation.
    ///
    /// The Provider adapter uses the same token for stream establishment and the
    /// returned producer lifetime, so cancellation remains live after `invoke` returns.
    pub cancellation: tokio_util::sync::CancellationToken,
    /// 本轮上下文窗口消息。
    pub messages: std::sync::Arc<[Message]>,
    /// 本轮 system prompt 块（provider-neutral）。
    pub system: Vec<RequestSystemBlockData>,
    /// 模型可见 tool schema 列表（wire-ready）。
    pub tools: Vec<serde_json::Value>,
    /// 单次调用最大输出 token。
    pub max_output_tokens: u32,
    /// 请求推理档位（clamp 前的原始请求值）。
    pub reasoning: ReasoningLevel,
}

impl InvocationRequestData {
    /// 构造一个最小请求（无 system、无 tools）。
    pub fn new(
        model: ModelIdData,
        messages: impl Into<std::sync::Arc<[Message]>>,
        max_output_tokens: u32,
        reasoning: ReasoningLevel,
    ) -> Self {
        Self {
            model,
            cancellation: tokio_util::sync::CancellationToken::new(),
            messages: messages.into(),
            system: Vec::new(),
            tools: Vec::new(),
            max_output_tokens,
            reasoning,
        }
    }
}

#[cfg(test)]
#[path = "published_language_tests.rs"]
mod tests;
