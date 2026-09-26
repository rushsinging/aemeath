//! Provider：LLM 上游的 HTTP/stream 实现与请求编排。
//!
//! # Published Language（四类语法，#1712 收敛·第一 PR）
//!
//! | 组 | 实体 |
//! |---|---|
//! | 工厂 | 零根工厂（判定：`composition` 构造面逻辑收进 provider 自有 wire 是本 issue 第二 PR——composition 不留复杂逻辑，wire 由 crate 提供） |
//! | 角色和职能 | `LlmProvider`（上游驱动 trait，Arc<dyn>）、`LlmClient`（具体客户端；composition 直包——同 ConfigAppService 判例，收窄随 #1696）、`TransportPool`（传输池构造面）、`CancellationSignal`（**归 #1739 信号统一，本批不动**） |
//! | 数据和生命周期 | 21 个 Data：`InvocationRequestData`（生命周期载体：含 cancellation 按值）、`InvocationStreamData`、`InvocationEventData`、`InvocationDeltaData`、`ModelIdData`、`ModelCapabilityData`、`ModelToolSchemaData`、`RequestSystemBlockData`、`SystemBlockData`、`InvocationScopeData`、`LlmConfigOptionsData`、`InvocationOptionsData`、`ProviderCompletionData`、`ProviderContentBlockData`、`ProviderToolCallData`、`ProviderToolCallIdData`、`ProviderStopReasonData`、`RawUsageSnapshotData`、`ReasoningCapabilityData`、`ReasoningMappingKindData` |
//! | Error | `ProviderError`（事实三字段；retryable/retry_after 迁出归 **#1740**）、`ProviderErrorKind`（13 变体）、`LlmError`（9 变体）——Error 类**不 Data 化** |
//!
//! 本批删除（零消费/转发）：`CapabilityFingerprint`、`RequestedInvocationOptions`、
//! `ResolvedInvocationOptions`、`ProviderDriverKind`（组内）、6 超时常量降 `pub(crate)`、
//! `ReasoningLevel` 转发（消费方直连 share::reasoning）。
//! 按 docs/design/03-engineering/05-published-language.md SOP。

//! LLM client library for aemeath.

#![deny(clippy::print_stdout, clippy::print_stderr)]

pub(crate) const LOG_TARGET: &str = "aemeath:agent:provider";

/// 本 crate 的日志 target。所有 log::xxx! 调用必须引用此常量。
mod adapters;
mod domain;
mod ports;
pub mod published_language;

pub(crate) use domain::capability::ProviderDriverKind;
pub(crate) use domain::invoke::InvocationScopeData;

/// Composition Root 专用构造面；业务消费者不得引用。
pub mod composition {
    pub use crate::adapters::client::{LlmClient, LlmConfigOptionsData};
    pub use crate::adapters::pool::TransportPool;
    pub use crate::domain::invoke::{InvocationScopeData, SystemBlockData};
    pub use crate::ports::LlmProvider;
    pub use crate::LlmError;
}
/// Provider HTTP 超时常量（crate 内装配用；跨 crate 零消费）。
pub(crate) const DEFAULT_TIMEOUT_SECS: u64 = 1800;
pub(crate) const CONNECT_TIMEOUT_SECS: u64 = 30;
pub(crate) const ANTHROPIC_STREAM_IDLE_TIMEOUT_SECS: u64 = 90;
pub(crate) const OPENAI_STREAM_IDLE_TIMEOUT_SECS: u64 = 180;
pub(crate) const OLLAMA_STREAM_IDLE_TIMEOUT_SECS: u64 = 180;
pub(crate) const STALL_THRESHOLD_SECS: u64 = 30;

pub use published_language::{
    CancellationSignal, InvocationDeltaData, InvocationEventData, InvocationOptionsData,
    InvocationRequestData, InvocationStreamData, ModelCapabilityData, ModelIdData,
    ModelToolSchemaData, ProviderCompletionData, ProviderContentBlockData, ProviderError,
    ProviderErrorKind, ProviderStopReasonData, ProviderToolCallData, ProviderToolCallIdData,
    RawUsageSnapshotData, ReasoningCapabilityData, ReasoningMappingKindData,
    RequestSystemBlockData,
};

#[derive(Debug, thiserror::Error)]
pub enum LlmError {
    #[error("network error: {0}")]
    Network(String),
    #[error("API error [{error_type}]: {message}")]
    Api { error_type: String, message: String },
    #[error("rate limited")]
    RateLimited,
    #[error("context too long")]
    ContextTooLong,
    #[error("request cancelled by user")]
    Cancelled,
    #[error("stream error: {0}")]
    Stream(String),
    #[error("stream connection interrupted: {0}")]
    StreamInterrupted(String),
    #[error("config error: {0}")]
    Config(String),
    #[error(
        "stream truncated mid-tool_call '{tool_call_name}' (id={tool_call_id}): {accumulated_bytes} bytes across {delta_count} deltas — provider closed SSE early"
    )]
    StreamTruncated {
        tool_call_id: String,
        tool_call_name: String,
        accumulated_bytes: usize,
        delta_count: u32,
        head_preview: String,
        tail_preview: String,
    },
}

impl LlmError {
    pub fn is_cancelled(&self) -> bool {
        matches!(self, LlmError::Cancelled)
    }

    pub fn is_stream_truncated(&self) -> bool {
        matches!(self, LlmError::StreamTruncated { .. })
    }
}

// ─── LlmError → ProviderError 权威映射（crate 内三 driver + stream + runtime 装配共用）───

impl From<LlmError> for ProviderError {
    fn from(error: LlmError) -> Self {
        let kind = match &error {
            LlmError::Cancelled => ProviderErrorKind::Cancelled,
            LlmError::RateLimited => ProviderErrorKind::RateLimited,
            LlmError::ContextTooLong => ProviderErrorKind::ContextTooLong,
            LlmError::Network(_) => ProviderErrorKind::Network,
            LlmError::Api { .. } => ProviderErrorKind::UpstreamUnavailable,
            LlmError::StreamInterrupted(_) | LlmError::StreamTruncated { .. } => {
                ProviderErrorKind::StreamTruncated
            }
            LlmError::Stream(_) => ProviderErrorKind::Protocol,
            LlmError::Config(_) => ProviderErrorKind::Configuration,
        };
        ProviderError::fatal(kind, error.to_string())
    }
}
#[cfg(test)]
mod tests {
    use super::LlmError;

    #[test]
    fn llm_cancelled_error_is_classified_as_cancelled() {
        assert!(LlmError::Cancelled.is_cancelled());
    }

    #[test]
    fn llm_stream_truncated_error_is_recognized_structurally() {
        let error = LlmError::StreamTruncated {
            tool_call_id: "call_x".to_string(),
            tool_call_name: "Write".to_string(),
            accumulated_bytes: 31428,
            delta_count: 3468,
            head_preview: "{\"file_path\":\"/x\"".to_string(),
            tail_preview: "...truncated...".to_string(),
        };
        assert!(error.is_stream_truncated());
        let rendered = format!("{error}");
        assert!(rendered.contains("Write"));
        assert!(rendered.contains("call_x"));
        assert!(rendered.contains("31428"));
    }

    #[test]
    fn llm_non_stream_truncated_errors_are_not_misclassified() {
        assert!(!LlmError::Cancelled.is_stream_truncated());
        assert!(!LlmError::Stream("some other failure".to_string()).is_stream_truncated());
        assert!(!LlmError::Network("reset".to_string()).is_stream_truncated());
        assert!(!LlmError::RateLimited.is_stream_truncated());
    }
}
