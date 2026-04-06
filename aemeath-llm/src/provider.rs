//! LLM Provider trait and common types

use async_trait::async_trait;
use aemeath_core::message::Message;
use tokio_util::sync::CancellationToken;

use crate::types::{StreamResponse, SystemBlock};

// Re-export Provider from aemeath_core
pub use aemeath_core::provider::Provider;

/// Handler trait for streaming responses
pub trait StreamHandler: Send {
    fn on_text(&mut self, text: &str);
    fn on_tool_use_start(&mut self, name: &str);
    fn on_error(&mut self, error: &str);
    fn on_raw_line(&mut self, _line: &str) {}
    fn on_text_block_complete(&mut self, _full_text: &str) {}
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
}