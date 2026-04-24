//! Unified LLM client that supports multiple providers

use std::sync::Arc;

use crate::provider::{CallbackHandler, LlmProvider, Provider, StreamHandler};
use crate::types::{StreamResponse, SystemBlock};
use aemeath_core::message::Message;
use tokio_util::sync::CancellationToken;

/// Truncate a string to at most `max_bytes`, snapping to the nearest char boundary
/// so we never split a multi-byte UTF-8 character.
fn truncate_preview(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &s[..end])
}

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
                    reasoning,
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
        self.log_request(system, messages, tool_schemas);
        let result = self.provider
            .stream_message(system, messages, tool_schemas, handler, cancel)
            .await;
        self.log_response(&result);
        result
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
        self.log_request(system, messages, tool_schemas);
        let mut handler = CallbackHandler::new(callback);
        let result = self.provider
            .stream_message(system, messages, tool_schemas, &mut handler, cancel)
            .await;
        self.log_response(&result);
        result
    }

    fn log_request(
        &self,
        system: &[SystemBlock],
        messages: &[Message],
        tool_schemas: &[serde_json::Value],
    ) {
        if !log::log_enabled!(log::Level::Debug) {
            return;
        }
        // Summarize messages: role + content-type hints (avoid dumping huge payloads)
        let msg_summary: Vec<serde_json::Value> = messages
            .iter()
            .enumerate()
            .map(|(i, msg)| {
                let blocks: Vec<serde_json::Value> = msg.content.iter().map(|block| {
                    match block {
                        aemeath_core::message::ContentBlock::Text { text } => {
                            let preview = truncate_preview(text, 200);
                            serde_json::json!({"type": "text", "preview": preview})
                        }
                        aemeath_core::message::ContentBlock::ToolUse { name, input, .. } => {
                            let input_str = input.to_string();
                            let preview = truncate_preview(&input_str, 300);
                            serde_json::json!({"type": "tool_use", "name": name, "input_preview": preview})
                        }
                        aemeath_core::message::ContentBlock::ToolResult { content, is_error, .. } => {
                            let s = content.to_string();
                            let preview = truncate_preview(&s, 300);
                            serde_json::json!({"type": "tool_result", "is_error": is_error, "preview": preview})
                        }
                        aemeath_core::message::ContentBlock::Thinking { thinking } => {
                            let preview = truncate_preview(thinking, 200);
                            serde_json::json!({"type": "thinking", "preview": preview})
                        }
                        aemeath_core::message::ContentBlock::Image { .. } => {
                            serde_json::json!({"type": "image", "preview": "[image data]"})
                        }
                    }
                }).collect();
                serde_json::json!({
                    "index": i,
                    "role": format!("{:?}", msg.role).to_lowercase(),
                    "blocks": blocks,
                })
            })
            .collect();

        let system_preview: Vec<String> = system.iter().map(|b| {
            truncate_preview(&b.text, 200)
        }).collect();

        log::debug!(
            "[LLM REQUEST] provider={} model={} system_blocks={} messages={} tools={}\n  system: {:?}\n  messages: {}",
            self.provider_name(),
            self.model_name(),
            system.len(),
            messages.len(),
            tool_schemas.len(),
            system_preview,
            serde_json::to_string_pretty(&msg_summary).unwrap_or_default(),
        );
    }

    fn log_response(&self, result: &Result<StreamResponse, crate::LlmError>) {
        if !log::log_enabled!(log::Level::Debug) {
            return;
        }
        match result {
            Ok(resp) => {
                let text = resp.assistant_message.text_content();
                let text_preview = truncate_preview(&text, 500);
                let tool_uses = resp.assistant_message.extract_tool_uses();
                let tools_summary: Vec<serde_json::Value> = tool_uses.iter().map(|(id, name, input)| {
                    let input_str = input.to_string();
                    let preview = truncate_preview(&input_str, 300);
                    serde_json::json!({"id": id, "name": name, "input_preview": preview})
                }).collect();

                log::debug!(
                    "[LLM RESPONSE] stop_reason={:?} input_tokens={} output_tokens={} tool_calls={}\n  text: {}\n  tools: {}",
                    resp.stop_reason,
                    resp.usage.input_tokens,
                    resp.usage.output_tokens,
                    tool_uses.len(),
                    text_preview,
                    serde_json::to_string_pretty(&tools_summary).unwrap_or_default(),
                );
            }
            Err(e) => {
                log::debug!(
                    "[LLM RESPONSE ERROR] {}",
                    e,
                );
            }
        }
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