//! LLM client library for aemeath
//!
//! Supports multiple LLM providers through a unified interface.

#![deny(clippy::print_stdout, clippy::print_stderr)]

pub mod api;
mod business;
pub mod contract;
mod core;
pub mod gateway;

pub use contract::ProviderDriverKind;

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
}

impl LlmError {
    pub fn is_cancelled(&self) -> bool {
        matches!(self, LlmError::Cancelled)
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
}
