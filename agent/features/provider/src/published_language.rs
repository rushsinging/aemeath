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

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::pin::Pin;
use std::time::Duration;

use async_trait::async_trait;
use futures_util::Stream;
use share::message::Message;

/// Provider 只读消费的取消信号。
///
/// 端口不暴露取消发起、child token 或 deadline；consumer drop 由私有
/// stream owner 负责转为 invocation-local 取消。
#[async_trait]
pub trait CancellationSignal: Send + Sync {
    fn is_cancelled(&self) -> bool;
    async fn cancelled(&self);
}

#[async_trait]
impl CancellationSignal for tokio_util::sync::CancellationToken {
    fn is_cancelled(&self) -> bool {
        tokio_util::sync::CancellationToken::is_cancelled(self)
    }

    async fn cancelled(&self) {
        tokio_util::sync::CancellationToken::cancelled(self).await;
    }
}

// ─── 模型标识 ───────────────────────────────────────────

/// 模型标识符（provider/model）。
///
/// 跨 BC 稳定标识一个 LLM 模型源，不携带 driver 或 transport 细节。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ModelId {
    /// provider 名称（如 "Anthropic"、"Zhipu"）。
    pub provider: String,
    /// 模型名称（如 "claude-sonnet-4-20250514"）。
    pub model: String,
}

impl std::fmt::Display for ModelId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.provider, self.model)
    }
}

// ─── Reasoning ──────────────────────────────────────────

/// Re-export ReasoningLevel from core::provider for PL consumers.
pub use crate::ports::ReasoningLevel;

/// Reasoning 映射方式——driver 如何把 ReasoningLevel 映射到 wire。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReasoningMappingKind {
    /// OpenAI 风格 effort 字符串。
    Effort,
    /// Anthropic 风格 thinking 开关。
    ThinkingToggle,
    /// Thinking budget（token 数）。
    ThinkingBudget,
    /// Adaptive 模式（provider 内部决定）。
    Adaptive,
    /// 不支持 reasoning。
    None,
}

/// 模型 reasoning 能力声明。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ReasoningCapability {
    supported: Vec<ReasoningLevel>,
    /// 映射方式。
    pub mapping: ReasoningMappingKind,
}

impl ReasoningCapability {
    pub fn new(
        supported: impl IntoIterator<Item = ReasoningLevel>,
        mapping: ReasoningMappingKind,
    ) -> Result<Self, ProviderError> {
        let mut supported: Vec<_> = supported.into_iter().collect();
        supported.sort_unstable();
        supported.dedup();
        if supported.first() != Some(&ReasoningLevel::Off) {
            return Err(ProviderError::fatal(
                ProviderErrorKind::Configuration,
                "reasoning capability 必须包含 off 档位",
            ));
        }
        Ok(Self { supported, mapping })
    }

    /// 构造不支持 reasoning 的默认能力。
    pub fn none() -> Self {
        Self {
            supported: vec![ReasoningLevel::Off],
            mapping: ReasoningMappingKind::None,
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

// ─── ModelCapability ────────────────────────────────────

/// 模型能力声明。
///
/// Runtime 可用于前置校验和展示；Provider 在请求编码前仍必须复核。
#[derive(Debug, Clone)]
pub struct ModelCapability {
    /// 模型标识。
    pub model: ModelId,
    /// 是否支持 tool use。
    pub supports_tools: bool,
    /// 是否支持并行 tool calls。
    pub supports_parallel_tool_calls: bool,
    /// 是否支持流式。
    pub supports_streaming: bool,
    /// Reasoning 能力。
    pub reasoning: ReasoningCapability,
    /// 上下文窗口大小（token 数），`None` 表示未知。
    pub context_limit: Option<usize>,
    /// 最大输出 token 数，`None` 表示未知。
    pub output_limit: Option<usize>,
}

impl ModelCapability {
    pub fn fingerprint(&self) -> CapabilityFingerprint {
        let mut hasher = DefaultHasher::new();
        self.model.hash(&mut hasher);
        self.supports_tools.hash(&mut hasher);
        self.supports_parallel_tool_calls.hash(&mut hasher);
        self.supports_streaming.hash(&mut hasher);
        self.reasoning.hash(&mut hasher);
        self.context_limit.hash(&mut hasher);
        self.output_limit.hash(&mut hasher);
        CapabilityFingerprint(hasher.finish())
    }

    pub fn resolve_invocation_options(
        &self,
        requested: RequestedInvocationOptions,
    ) -> Result<ResolvedInvocationOptions, ProviderError> {
        if requested.context_size == 0
            || requested.context_size == usize::MAX && self.context_limit.is_none()
            || requested.max_output_tokens == 0
        {
            return Err(ProviderError::fatal(
                ProviderErrorKind::Configuration,
                "invocation token limit 必须大于零",
            ));
        }
        let context_size = self.context_limit.unwrap_or(requested.context_size);
        let max_output_tokens = self
            .output_limit
            .map_or(requested.max_output_tokens, |limit| {
                requested.max_output_tokens.min(limit)
            });
        Ok(ResolvedInvocationOptions {
            context_size,
            max_output_tokens,
            requested_reasoning: requested.reasoning,
            effective_reasoning: self.reasoning.resolve(requested.reasoning),
            capability_fingerprint: self.fingerprint(),
        })
    }
}

// ─── Tool Call（Provider 边界） ─────────────────────────

/// Provider 边界的 tool call 标识。
///
/// 这是 Provider 返回的原始 tool-call ID（如 Anthropic 的 `toolu_*` 或
/// OpenAI 的 `call_*`）。Runtime 在写入 Run Step 时创建领域 `ToolCallId`
/// 并维护双 ID 映射。Provider **NEVER** 生成领域 ID。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderToolCallId(pub String);

impl std::fmt::Display for ProviderToolCallId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Provider 边界的 tool call 完整形态。
#[derive(Debug, Clone)]
pub struct ProviderToolCall {
    /// Provider 原始 tool-call ID。
    pub id: ProviderToolCallId,
    /// 工具名称。
    pub name: String,
    /// 验证过的 JSON 参数。
    pub arguments: serde_json::Value,
}

/// Provider 返回的 assistant 内容块。
#[derive(Debug, Clone)]
pub enum ProviderContentBlock {
    /// 文本内容。
    Text(String),
    /// Thinking/reasoning 内容（签名可选）。
    Thinking {
        thinking: String,
        signature: Option<String>,
    },
    /// Tool call。
    ToolCall(ProviderToolCall),
}

// ─── Raw Usage ──────────────────────────────────────────

/// 原始 token 使用快照。
///
/// 所有字段区分"未报告"（`None`）与真实零值（`Some(0)`）。
/// Provider 只做协议标准化，不计算 cost。
#[derive(Debug, Clone, Default)]
pub struct RawUsageSnapshot {
    pub input_tokens: Option<u32>,
    pub output_tokens: Option<u32>,
    pub cache_read_tokens: Option<u32>,
    pub cache_write_tokens: Option<u32>,
    pub reasoning_tokens: Option<u32>,
}

// ─── Stop Reason ────────────────────────────────────────

/// 统一停止原因。
///
/// 注意：与 legacy `business::types::StopReason`（3 变体）不同。
/// 对外 re-export 时使用别名 `ProviderStopReason` 以避免命名冲突。
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
pub use StopReason as ProviderStopReason;

// ─── ProviderCompletion ─────────────────────────────────

/// 一次调用的终结完成态。
///
/// `output` 必须是所有已发 delta 的完整最终形态，
/// 并保留 provider tool-call ID。
#[derive(Debug, Clone)]
pub struct ProviderCompletion {
    /// 最终 assistant 内容块。
    pub output: Vec<ProviderContentBlock>,
    /// 停止原因。
    pub stop_reason: StopReason,
    /// 最终 usage 快照（`None` = provider 未返回 usage）。
    pub usage: Option<RawUsageSnapshot>,
    /// 有效 reasoning level（clamp 后的实际档位）。
    pub effective_reasoning: ReasoningLevel,
}

// ─── Invocation Delta ───────────────────────────────────

/// 流式 delta——非终结增量。
///
/// 终结增量通过 `InvocationEvent::Completed` / `InvocationEvent::Failed` 表达，
/// 不出现在 `InvocationDelta` 中。
#[derive(Debug, Clone)]
pub enum InvocationDelta {
    /// 文本增量。
    Text(String),
    /// Thinking/reasoning 增量。
    Thinking {
        thinking: String,
        signature: Option<String>,
    },
    /// Tool call 开始。
    ToolCallStarted {
        index: usize,
        provider_id: Option<ProviderToolCallId>,
        name: String,
    },
    /// Tool arguments 增量字符串片段。
    ToolArgumentsDelta {
        index: usize,
        provider_id: Option<ProviderToolCallId>,
        partial_json: String,
    },
    /// Tool call 完成（给出验证过的 JSON 值）。
    ToolCallCompleted {
        index: usize,
        call: ProviderToolCall,
    },
    /// Usage 快照更新。
    UsageSnapshot(RawUsageSnapshot),
}

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

// ─── Model Tool Schema ──────────────────────────────────

/// 模型可见的 tool schema。
///
/// 这是 Tool Catalog 的模型可见投影。driver 转换时只保留供应商允许字段。
#[derive(Debug, Clone)]
pub struct ModelToolSchema {
    /// 工具名称。
    pub name: String,
    /// 工具描述。
    pub description: String,
    /// 输入 JSON schema。
    pub input_schema: serde_json::Value,
}

// ─── InvocationOptions ──────────────────────────────────

/// 同一进程内检测 capability 变化的指纹；NEVER 持久化或跨构建比较。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CapabilityFingerprint(u64);

impl CapabilityFingerprint {
    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RequestedInvocationOptions {
    context_size: usize,
    max_output_tokens: usize,
    reasoning: ReasoningLevel,
}

impl RequestedInvocationOptions {
    /// 构造请求；若 capability 未声明 context limit，调用方必须用
    /// `with_context_size` 提供实际 fallback，resolver 会拒绝未设置值。
    pub fn new(max_output_tokens: usize, reasoning: ReasoningLevel) -> Self {
        Self {
            context_size: usize::MAX,
            max_output_tokens,
            reasoning,
        }
    }

    pub fn with_context_size(mut self, context_size: usize) -> Self {
        self.context_size = context_size;
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedInvocationOptions {
    context_size: usize,
    max_output_tokens: usize,
    requested_reasoning: ReasoningLevel,
    effective_reasoning: ReasoningLevel,
    capability_fingerprint: CapabilityFingerprint,
}

impl ResolvedInvocationOptions {
    pub const fn context_size(&self) -> usize {
        self.context_size
    }

    pub const fn max_output_tokens(&self) -> usize {
        self.max_output_tokens
    }

    pub const fn requested_reasoning(&self) -> ReasoningLevel {
        self.requested_reasoning
    }

    pub const fn effective_reasoning(&self) -> ReasoningLevel {
        self.effective_reasoning
    }

    pub const fn capability_fingerprint(&self) -> CapabilityFingerprint {
        self.capability_fingerprint
    }
}

/// Legacy 一次调用选项；生产 resolver 接线延期到 v0.2.0 决策。
#[derive(Debug, Clone)]
pub struct InvocationOptions {
    /// 最大输出 token。
    pub max_output_tokens: u32,
    /// 期望 reasoning level（Workflow 已应用 Config 静态上限）。
    pub reasoning: ReasoningLevel,
}

impl InvocationOptions {
    /// 构造默认选项。
    pub fn new(max_output_tokens: u32, reasoning: ReasoningLevel) -> Self {
        Self {
            max_output_tokens,
            reasoning,
        }
    }
}

// ─── InvocationRequest ──────────────────────────────────

/// 一次 LLM 调用请求。
///
/// 一个 `InvocationRequest` 固定一个 model 和一份不可变 options。
#[derive(Debug, Clone)]
pub struct InvocationRequest {
    /// 目标模型。
    pub model: ModelId,
    /// 本轮上下文窗口消息。
    pub messages: Vec<Message>,
    /// 模型可见 tool schema 列表。
    pub tools: Vec<ModelToolSchema>,
    /// 调用选项。
    pub options: InvocationOptions,
}

impl InvocationRequest {
    /// 构造一个最小请求（无 tools）。
    pub fn new(model: ModelId, messages: Vec<Message>, options: InvocationOptions) -> Self {
        Self {
            model,
            messages,
            tools: Vec::new(),
            options,
        }
    }
}

// ─── InvocationEvent ────────────────────────────────────

/// 一次调用的流式事件。
///
/// Delta 是非终结增量；Completed 和 Failed 是互斥终结事件，恰好出现一个。
/// 取消以 `Failed(ProviderError::cancelled())` 终结。
/// 终结事件后下一次 `next()` 返回 `None`。
#[derive(Debug, Clone)]
pub enum InvocationEvent {
    /// 非终结增量。
    Delta(InvocationDelta),
    /// 完成终结（恰好出现一次）。
    Completed(ProviderCompletion),
    /// 失败终结（恰好出现一次，取消也归入此变体）。
    Failed(ProviderError),
}

impl InvocationEvent {
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Completed(_) | Self::Failed(_))
    }
}

/// 一次上游语义请求的有序 pull stream。
pub type InvocationStream = Pin<Box<dyn Stream<Item = InvocationEvent> + Send>>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_id_display() {
        let id = ModelId {
            provider: "Anthropic".to_string(),
            model: "claude-sonnet-4".to_string(),
        };
        assert_eq!(id.to_string(), "Anthropic/claude-sonnet-4");
    }

    #[test]
    fn provider_error_cancelled() {
        let e = ProviderError::cancelled();
        assert!(e.is_cancelled());
        assert!(!e.retryable);
        assert_eq!(e.kind, ProviderErrorKind::Cancelled);
    }

    #[test]
    fn provider_error_context_exceeded() {
        let e = ProviderError::fatal(ProviderErrorKind::ContextTooLong, "context length exceeded");
        assert!(e.is_context_exceeded());
        assert!(!e.retryable);
    }

    #[test]
    fn provider_error_retryable() {
        let e = ProviderError::retryable(ProviderErrorKind::RateLimited, "429");
        assert!(e.retryable);
        assert_eq!(e.kind, ProviderErrorKind::RateLimited);
    }

    #[test]
    fn reasoning_capability_none() {
        let cap = ReasoningCapability::none();
        assert_eq!(cap.supported(), &[ReasoningLevel::Off]);
        assert_eq!(cap.maximum(), ReasoningLevel::Off);
        assert_eq!(cap.mapping, ReasoningMappingKind::None);
    }

    #[test]
    fn resolver_selects_highest_supported_level_not_above_requested() {
        let capability = ModelCapability {
            model: ModelId {
                provider: "fake".to_string(),
                model: "sparse-levels".to_string(),
            },
            supports_tools: true,
            supports_parallel_tool_calls: true,
            supports_streaming: true,
            reasoning: ReasoningCapability::new(
                [
                    ReasoningLevel::Off,
                    ReasoningLevel::Medium,
                    ReasoningLevel::Max,
                ],
                ReasoningMappingKind::Effort,
            )
            .expect("valid sparse capability"),
            context_limit: Some(128_000),
            output_limit: Some(8_192),
        };

        for (requested, expected) in [
            (ReasoningLevel::Off, ReasoningLevel::Off),
            (ReasoningLevel::Low, ReasoningLevel::Off),
            (ReasoningLevel::Medium, ReasoningLevel::Medium),
            (ReasoningLevel::High, ReasoningLevel::Medium),
            (ReasoningLevel::Xhigh, ReasoningLevel::Medium),
            (ReasoningLevel::Max, ReasoningLevel::Max),
        ] {
            let resolved = capability
                .resolve_invocation_options(RequestedInvocationOptions::new(16_384, requested))
                .expect("capability should resolve");
            assert_eq!(resolved.requested_reasoning(), requested);
            assert_eq!(resolved.effective_reasoning(), expected);
            assert!(resolved.effective_reasoning() <= resolved.requested_reasoning());
            assert_eq!(resolved.context_size(), 128_000);
            assert_eq!(resolved.max_output_tokens(), 8_192);
        }
    }

    #[test]
    fn capability_fingerprint_is_stable_and_changes_with_semantics() {
        let model = ModelId {
            provider: "fake".to_string(),
            model: "fingerprinted".to_string(),
        };
        let base = ModelCapability {
            model: model.clone(),
            supports_tools: true,
            supports_parallel_tool_calls: false,
            supports_streaming: true,
            reasoning: ReasoningCapability::new(
                [ReasoningLevel::Off, ReasoningLevel::High],
                ReasoningMappingKind::Effort,
            )
            .unwrap(),
            context_limit: Some(100_000),
            output_limit: Some(4_096),
        };
        let same = base.clone();
        let mut changed = base.clone();
        changed.reasoning = ReasoningCapability::new(
            [ReasoningLevel::Off, ReasoningLevel::Medium],
            ReasoningMappingKind::Effort,
        )
        .unwrap();

        assert_eq!(base.fingerprint(), same.fingerprint());
        assert_ne!(base.fingerprint(), changed.fingerprint());
    }

    #[test]
    fn reasoning_capability_rejects_empty_or_missing_off_levels() {
        assert!(ReasoningCapability::new([], ReasoningMappingKind::None).is_err());
        assert!(
            ReasoningCapability::new([ReasoningLevel::Medium], ReasoningMappingKind::Effort,)
                .is_err()
        );
    }

    #[test]
    fn stop_reason_variants() {
        let r = StopReason::EndTurn;
        assert_eq!(r, StopReason::EndTurn);

        let other = StopReason::Other("unknown".to_string());
        assert!(matches!(other, StopReason::Other(_)));
    }

    #[test]
    fn provider_tool_call_id_display() {
        let id = ProviderToolCallId("toolu_123".to_string());
        assert_eq!(id.to_string(), "toolu_123");
    }

    #[test]
    fn raw_usage_snapshot_default_all_none() {
        let usage = RawUsageSnapshot::default();
        assert!(usage.input_tokens.is_none());
        assert!(usage.output_tokens.is_none());
        assert!(usage.cache_read_tokens.is_none());
    }

    #[test]
    fn invocation_request_new_has_empty_tools() {
        let req = InvocationRequest::new(
            ModelId {
                provider: "test".to_string(),
                model: "m".to_string(),
            },
            Vec::new(),
            InvocationOptions::new(8192, ReasoningLevel::Off),
        );
        assert!(req.tools.is_empty());
    }

    #[test]
    fn invocation_event_delta_is_non_terminal() {
        let evt = InvocationEvent::Delta(InvocationDelta::Text("hi".to_string()));
        assert!(!evt.is_terminal());
    }

    #[test]
    fn invocation_event_completed_and_failed_are_terminal() {
        let completion = ProviderCompletion {
            output: Vec::new(),
            stop_reason: StopReason::EndTurn,
            usage: None,
            effective_reasoning: ReasoningLevel::Off,
        };
        assert!(InvocationEvent::Completed(completion).is_terminal());
        assert!(InvocationEvent::Failed(ProviderError::cancelled()).is_terminal());
    }

    #[test]
    fn tool_call_identity_can_bind_provider_id_after_start() {
        let started = InvocationDelta::ToolCallStarted {
            index: 2,
            provider_id: None,
            name: "Write".to_string(),
        };
        let arguments = InvocationDelta::ToolArgumentsDelta {
            index: 2,
            provider_id: Some(ProviderToolCallId("call_late".to_string())),
            partial_json: "{}".to_string(),
        };

        assert!(matches!(
            started,
            InvocationDelta::ToolCallStarted {
                index: 2,
                provider_id: None,
                ..
            }
        ));
        assert!(matches!(
            arguments,
            InvocationDelta::ToolArgumentsDelta {
                index: 2,
                provider_id: Some(ProviderToolCallId(ref id)),
                ..
            } if id == "call_late"
        ));
    }

    #[tokio::test]
    async fn cancellation_token_implements_object_safe_signal() {
        fn assert_object_safe(_: &dyn CancellationSignal) {}

        let token = tokio_util::sync::CancellationToken::new();
        let signal: &dyn CancellationSignal = &token;
        assert_object_safe(signal);
        assert!(!signal.is_cancelled());
        token.cancel();
        signal.cancelled().await;
        assert!(signal.is_cancelled());
    }
}
