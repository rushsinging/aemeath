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

// ─── ModelInfo ─────────────────────────────────────────

/// 模型元数据——config/catalog 外部数据的单一实体投影。
///
/// provider 是读侧（不写入）；身份（provider/model）与能力（supports_*、
/// reasoning、调用限制）不可分：不存在脱离能力的纯标识场景。
#[derive(Debug, Clone, PartialEq)]
pub struct ModelInfo {
    /// provider 名称（如 "Anthropic"）。
    pub provider: String,
    /// 模型名。
    pub model: String,
    /// 是否支持 tool use。
    pub supports_tools: bool,
    /// 是否支持并行 tool calls。
    pub supports_parallel_tool_calls: bool,
    /// 是否支持流式。
    pub supports_streaming: bool,
    /// Reasoning 支持阶梯（升序去重；由 client 装配时按 driver 能力覆盖）。
    pub supported_reasoning: Vec<ReasoningLevel>,
    /// 上下文窗口大小（token 数），`None` 表示未知。
    pub context_limit: Option<usize>,
    /// 最大输出 token 数，`None` 表示未知。
    pub output_limit: Option<usize>,
}

impl ModelInfo {
    /// 请求档位在支持阶梯内的 clamp：阶梯中 ≤requested 的最大档，Off 兜底。
    ///
    /// 原 reasoning 能力数据类型的 resolve 逻辑（#1861 v4 摊平进实体）。
    pub fn resolve_reasoning(&self, requested: ReasoningLevel) -> ReasoningLevel {
        crate::domain::capability::resolve_supported(&self.supported_reasoning, requested)
    }
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
/// 对外 re-export 时使用别名 `ResponseStopReason` 以避免命名冲突。
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
pub use StopReason as ResponseStopReason;

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
    pub stop_reason: Option<ResponseStopReason>,
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
    Stop(ResponseStopReason),
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
    ///
    /// ID 为 Provider 返回的原始 tool-call 标识（如 Anthropic 的 `toolu_*` 或
    /// OpenAI 的 `call_*`）。Runtime 在写入 Run Step 时创建领域 `ToolCallId`
    /// 并维护双 ID 映射。Provider **NEVER** 生成领域 ID。
    ToolCall {
        /// Provider 原始 tool-call ID。
        id: String,
        /// 工具名称。
        name: String,
        /// 验证过的 JSON 参数。
        arguments: serde_json::Value,
    },
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
        /// Provider 原始 tool-call ID。
        id: String,
        /// 工具名称。
        name: String,
        /// 验证过的 JSON 参数。
        arguments: serde_json::Value,
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

// ─── ProviderRequestData ──────────────────────────────────

/// 一次 LLM 调用请求。
///
/// 一个 `ProviderRequestData` 固定一个 model 和一份不可变 options。
#[derive(Debug, Clone)]
pub struct ProviderRequestData {
    /// 目标模型名（binding 已绑定具体客户端与 [`ModelInfo`]，请求只携带名字）。
    pub model: String,
    /// Runtime-owned cancellation token for this invocation.
    ///
    /// The Provider adapter uses the same token for stream establishment and the
    /// returned producer lifetime, so cancellation remains live after `invoke` returns.
    pub cancellation: tokio_util::sync::CancellationToken,
    /// 本轮上下文窗口消息。
    pub messages: std::sync::Arc<[Message]>,
    /// 本轮 system prompt——context/runtime 拼好的整段文本（#1861 v4：
    /// 原块级 system 数据类型消除，拼接职责归上游）。
    pub system: String,
    /// 可缓存前缀的字节长度（prompt caching 分界；0 = 无）。
    ///
    /// 分界天然落在原块边界上（`llm_strategy` 拼接时按 `cache_break` 计算），
    /// 因此按字节切分不会切断 UTF-8 字符。
    pub static_prefix_len: usize,
    /// 模型可见 tool schema 列表（wire-ready）。
    pub tools: Vec<serde_json::Value>,
    /// 单次调用最大输出 token。
    pub max_output_tokens: u32,
    /// 请求推理档位（clamp 前的原始请求值）。
    pub reasoning: ReasoningLevel,
}

impl ProviderRequestData {
    /// 构造一个最小请求（无 system、无 tools）。
    pub fn new(
        model: String,
        messages: impl Into<std::sync::Arc<[Message]>>,
        max_output_tokens: u32,
        reasoning: ReasoningLevel,
    ) -> Self {
        Self {
            model,
            cancellation: tokio_util::sync::CancellationToken::new(),
            messages: messages.into(),
            system: String::new(),
            static_prefix_len: 0,
            tools: Vec::new(),
            max_output_tokens,
            reasoning,
        }
    }
}

#[cfg(test)]
#[path = "published_language_tests.rs"]
mod tests;
