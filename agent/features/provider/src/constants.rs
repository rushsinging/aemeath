//! Provider crate 身份与 HTTP 超时常量（#1146 双轨归位）。

pub(crate) const LOG_TARGET: &str = "aemeath:agent:provider";

/// Provider HTTP 超时常量（crate 内装配用；跨 crate 零消费）。
pub(crate) const CONNECT_TIMEOUT_SECS: u64 = 30;
pub(crate) const ANTHROPIC_STREAM_IDLE_TIMEOUT_SECS: u64 = 90;
pub(crate) const OPENAI_STREAM_IDLE_TIMEOUT_SECS: u64 = 180;
pub(crate) const OLLAMA_STREAM_IDLE_TIMEOUT_SECS: u64 = 180;
pub(crate) const STALL_THRESHOLD_SECS: u64 = 30;
