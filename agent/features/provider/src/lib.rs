//! LLM client library for aemeath.

#![deny(clippy::print_stdout, clippy::print_stderr)]

pub(crate) const LOG_TARGET: &str = "aemeath:agent:provider";

/// 本 crate 的日志 target。所有 log::xxx! 调用必须引用此常量。
mod adapters;
mod domain;
mod ports;
pub mod published_language;

pub(crate) use domain::capability::ProviderDriverKind;
pub use domain::capability::ReasoningLevel;
pub(crate) use domain::invoke::InvocationScope;

/// Composition Root 专用构造面；业务消费者不得引用。
pub mod composition {
    pub use crate::adapters::client::{LlmClient, LlmConfigOptions};
    pub use crate::domain::capability::ProviderDriverKind;
    pub use crate::domain::invoke::{InvocationScope, SystemBlock};
    pub use crate::ports::LlmProvider;
    pub use crate::LlmError;
}
/// Test-only concrete Provider compatibility surface for migration fixtures.
#[cfg(feature = "test-harness")]
pub mod test_harness {
    pub use crate::adapters::client::LlmClient;
    pub use crate::domain::invoke::{InvocationScope, SystemBlock};
    pub use crate::ports::LlmProvider;
}

pub use published_language::{
    CancellationSignal, CapabilityFingerprint, InvocationDelta, InvocationEvent, InvocationOptions,
    InvocationRequest, InvocationStream, ModelCapability, ModelId, ModelToolSchema,
    ProviderCompletion, ProviderContentBlock, ProviderError, ProviderErrorKind, ProviderStopReason,
    ProviderToolCall, ProviderToolCallId, RawUsageSnapshot, ReasoningCapability,
    ReasoningMappingKind, RequestSystemBlock, RequestedInvocationOptions,
    ResolvedInvocationOptions,
};

/// Provider HTTP 超时常量。
pub const DEFAULT_TIMEOUT_SECS: u64 = 1800;
pub const CONNECT_TIMEOUT_SECS: u64 = 30;
pub const ANTHROPIC_STREAM_IDLE_TIMEOUT_SECS: u64 = 90;
pub const OPENAI_STREAM_IDLE_TIMEOUT_SECS: u64 = 180;
pub const OLLAMA_STREAM_IDLE_TIMEOUT_SECS: u64 = 180;
pub const STALL_THRESHOLD_SECS: u64 = 30;

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
