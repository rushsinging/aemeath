//! LLM client library for aemeath
//!
//! Supports multiple LLM providers through a unified interface.

#![deny(clippy::print_stdout, clippy::print_stderr)]

/// 本 crate 的日志 target。所有 log::xxx! 调用必须引用此常量。
pub const LOG_TARGET: &str = "aemeath:agent:provider";

pub mod api;
mod business;
pub mod contract;
mod core;
pub mod gateway;

pub use contract::ProviderDriverKind;

/// Provider HTTP 超时常量（单一真相源，见 `business` 模块）。
pub use business::{
    ANTHROPIC_STREAM_IDLE_TIMEOUT_SECS, CONNECT_TIMEOUT_SECS, DEFAULT_TIMEOUT_SECS,
    OLLAMA_STREAM_IDLE_TIMEOUT_SECS, OPENAI_STREAM_IDLE_TIMEOUT_SECS, STALL_THRESHOLD_SECS,
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
    #[error("config error: {0}")]
    Config(String),
    /// 上游 SSE 流在某个 tool_call 的 JSON arguments 字符串中间被截断。
    /// 用结构化字段替代"通过字符串嗅探判断"的做法，方便 caller 精确路由。
    #[error(
        "stream truncated mid-tool_call '{tool_call_name}' (id={tool_call_id}): \
         {accumulated_bytes} bytes across {delta_count} deltas — provider closed SSE early"
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

    /// 是否属于"上游 SSE 流在 tool_call 中间被截断"的稳定失败模式。
    /// 替代先前 `e.contains("upstream truncated")` 的字符串嗅探。
    pub fn is_stream_truncated(&self) -> bool {
        matches!(self, LlmError::StreamTruncated { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::LlmError;

    #[test]
    fn llm_cancelled_error_is_classified_as_cancelled() {
        let error = LlmError::Cancelled;
        assert!(error.is_cancelled());
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
        // Display 包含关键诊断字段，方便日志追溯
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
