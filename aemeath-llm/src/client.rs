//! Unified LLM client that supports multiple providers

use std::sync::Arc;

use crate::provider::{CallbackHandler, LlmProvider, Provider, StreamHandler};
use crate::types::{StreamResponse, SystemBlock};
use aemeath_core::message::Message;
use tokio_util::sync::CancellationToken;

/// Unified LLM client that wraps different providers
pub struct LlmClient {
    provider: Arc<dyn LlmProvider>,
}

impl LlmClient {
    /// Create a new LLM client with Anthropic provider (default)
    pub fn new(api_key: String) -> Self {
        Self::with_provider(Provider::Anthropic, api_key, None, None, 200000, false)
    }

    /// Create a new LLM client with specified provider
    pub fn with_provider(
        provider: Provider,
        api_key: String,
        base_url: Option<String>,
        model: Option<String>,
        max_tokens: u32,
        reasoning: bool,
    ) -> Self {
        let provider_impl: Arc<dyn LlmProvider> = match provider {
            Provider::Anthropic => {
                Arc::new(crate::providers::AnthropicProvider::new(
                    api_key,
                    base_url,
                    model,
                    max_tokens,
                ))
            }
            Provider::Ollama => {
                Arc::new(crate::providers::OllamaProvider::new(
                    api_key,
                    base_url,
                    model,
                    max_tokens,
                ))
            }
            Provider::OpenAI
            | Provider::OpenRouter
            | Provider::DeepSeek
            | Provider::Moonshot
            | Provider::Zhipu
            | Provider::DashScope
            | Provider::MiniMax
            | Provider::OpenAICompatible => {
                Arc::new(crate::providers::OpenAICompatibleProvider::new(
                    provider,
                    api_key,
                    base_url,
                    model,
                    max_tokens,
                    reasoning,
                ))
            }
        };
        Self {
            provider: provider_impl,
        }
    }

    /// Create LlmClient from configuration
    pub fn from_config(
        provider: Provider,
        api_key: String,
        base_url: Option<String>,
        model: String,
        max_tokens: u32,
        reasoning: bool,
    ) -> Self {
        Self::with_provider(provider, api_key, base_url, Some(model), max_tokens, reasoning)
    }

    /// Stream a message with tool support
    pub async fn stream_message(
        &self,
        system: &[SystemBlock],
        messages: &[Message],
        tool_schemas: &[serde_json::Value],
        handler: &mut dyn StreamHandler,
        cancel: &CancellationToken,
    ) -> Result<StreamResponse, crate::LlmError> {
        self.provider
            .stream_message(system, messages, tool_schemas, handler, cancel)
            .await
    }

    /// Stream message with a simple text callback (for TUI)
    pub async fn stream_message_raw(
        &self,
        system: &[SystemBlock],
        messages: &[Message],
        tool_schemas: &[serde_json::Value],
        callback: Box<dyn FnMut(&str) + Send>,
        cancel: &CancellationToken,
    ) -> Result<StreamResponse, crate::LlmError> {
        let mut handler = CallbackHandler::new(callback);
        self.provider
            .stream_message(system, messages, tool_schemas, &mut handler, cancel)
            .await
    }

    /// Get the model name
    pub fn model_name(&self) -> &str {
        self.provider.model_name()
    }

    /// Get the provider name
    pub fn provider_name(&self) -> &str {
        self.provider.provider_name()
    }
}