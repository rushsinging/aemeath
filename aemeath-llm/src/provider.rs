//! LLM Provider trait and common types

use async_trait::async_trait;
use aemeath_core::message::Message;
use tokio_util::sync::CancellationToken;

use crate::types::{StreamResponse, SystemBlock};

// Re-export Provider from aemeath_core
pub use aemeath_core::provider::ApiType;

/// Handler trait for streaming responses
pub trait StreamHandler: Send {
    fn on_text(&mut self, text: &str);
    fn on_tool_use_start(&mut self, name: &str);
    fn on_error(&mut self, error: &str);
    fn on_raw_line(&mut self, _line: &str) {}
    fn on_text_block_complete(&mut self, _full_text: &str) {}
    /// Called for reasoning/thinking content (e.g. GLM-5.1, DeepSeek-R1).
    /// Default: ignored. Override to display thinking in a special style.
    fn on_thinking(&mut self, _text: &str) {}
}

/// Simple callback handler for raw text streaming
pub struct CallbackHandler {
    callback: Box<dyn FnMut(&str) + Send>,
}

impl CallbackHandler {
    pub fn new(callback: Box<dyn FnMut(&str) + Send>) -> Self {
        Self { callback }
    }
}

impl StreamHandler for CallbackHandler {
    fn on_text(&mut self, text: &str) {
        (self.callback)(text);
    }
    fn on_tool_use_start(&mut self, _name: &str) {}
    fn on_error(&mut self, _error: &str) {}
    fn on_text_block_complete(&mut self, _full_text: &str) {}
}

/// LLM Provider trait - all providers must implement this
#[async_trait]
pub trait LlmProvider: Send + Sync {
    /// Stream a message with tool support
    async fn stream_message(
        &self,
        system: &[SystemBlock],
        messages: &[Message],
        tool_schemas: &[serde_json::Value],
        handler: &mut dyn StreamHandler,
        cancel: &CancellationToken,
    ) -> Result<StreamResponse, crate::LlmError>;

    /// Get the model name
    fn model_name(&self) -> &str;

    /// Get the provider name
    fn provider_name(&self) -> &str;

    /// Set reasoning/thinking mode at runtime
    fn set_reasoning(&self, enabled: bool);

    /// Get current reasoning/thinking mode
    fn is_reasoning(&self) -> bool;

    /// Set reasoning_effort level (e.g. "low", "medium", "high") at runtime.
    /// Ignored by providers that don't support it.
    fn set_reasoning_effort(&self, _effort: Option<String>) {}

    /// Get current reasoning_effort level.
    fn reasoning_effort(&self) -> Option<String> { None }
}