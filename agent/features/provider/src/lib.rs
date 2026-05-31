//! LLM client library for aemeath
//!
//! Supports multiple LLM providers through a unified interface.

#![deny(clippy::print_stdout, clippy::print_stderr)]

pub mod api;
pub mod contract;
pub mod gateway;
mod business;
mod core;

pub use contract::ApiDriverKind;

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
