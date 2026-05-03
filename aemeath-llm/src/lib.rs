//! LLM client library for aemeath
//!
//! Supports multiple LLM providers through a unified interface.

#![deny(clippy::print_stdout, clippy::print_stderr)]

pub mod client;
pub mod pool;
pub mod provider;
pub mod providers;
pub mod stream;
pub mod types;

pub use client::LlmClient;
pub use pool::LlmClientPool;
pub use provider::{ApiType, CallbackHandler, LlmProvider, StreamHandler};
pub use providers::{AnthropicProvider, OpenAICompatibleProvider};

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
    #[error("stream error: {0}")]
    Stream(String),
    #[error("config error: {0}")]
    Config(String),
}
