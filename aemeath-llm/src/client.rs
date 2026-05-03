//! Unified LLM client that supports multiple providers

use std::sync::Arc;

use crate::provider::{CallbackHandler, LlmProvider, StreamHandler};
use crate::providers::openai_compatible::ReasoningConfig;
use crate::types::{StreamResponse, SystemBlock};
use aemeath_core::message::Message;
use aemeath_core::provider::ApiDriverKind;
use tokio_util::sync::CancellationToken;

/// Truncate a string to at most `max_bytes`, snapping to the nearest char boundary.
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

/// Configuration for OpenAI-compatible providers. The source key is used only
/// for display/logging; API behavior comes from `api`.
#[derive(Debug, Clone)]
pub struct OpenAIProviderConfig {
    pub source_key: String,
    pub api: ApiDriverKind,
    pub chat_api_suffix: String,
}

impl OpenAIProviderConfig {
    pub fn from_api_driver(api: ApiDriverKind, source_key: &str) -> Self {
        Self {
            source_key: source_key.to_string(),
            api,
            chat_api_suffix: match api {
                ApiDriverKind::Zhipu => "/chat/completions".to_string(),
                ApiDriverKind::Anthropic => "/v1/messages".to_string(),
                ApiDriverKind::OpenAI | ApiDriverKind::LiteLLM => {
                    "/v1/chat/completions".to_string()
                }
            },
        }
    }
}

pub struct LlmClient {
    provider: Arc<dyn LlmProvider>,
}

impl LlmClient {
    pub fn new(api_key: String) -> Self {
        Self::with_provider(
            ApiDriverKind::Anthropic,
            api_key,
            None,
            None,
            200000,
            false,
            None,
        )
    }

    pub fn with_provider(
        api: ApiDriverKind,
        api_key: String,
        base_url: Option<String>,
        model: Option<String>,
        max_tokens: u32,
        reasoning: bool,
        reasoning_config: Option<ReasoningConfig>,
    ) -> Self {
        let provider_impl: Arc<dyn LlmProvider> = match api {
            ApiDriverKind::Anthropic => Arc::new(crate::providers::AnthropicProvider::new(
                api_key, base_url, model, max_tokens,
            )),
            ApiDriverKind::OpenAI | ApiDriverKind::Zhipu | ApiDriverKind::LiteLLM => {
                let config = OpenAIProviderConfig::from_api_driver(api, api.as_str());
                Arc::new(crate::providers::OpenAICompatibleProvider::new(
                    config,
                    api_key,
                    base_url,
                    model,
                    max_tokens,
                    reasoning,
                    reasoning_config,
                ))
            }
        };
        Self {
            provider: provider_impl,
        }
    }

    pub fn with_openai_config(
        config: OpenAIProviderConfig,
        api_key: String,
        base_url: Option<String>,
        model: Option<String>,
        max_tokens: u32,
        reasoning: bool,
        reasoning_config: Option<ReasoningConfig>,
    ) -> Self {
        let provider_impl: Arc<dyn LlmProvider> =
            Arc::new(crate::providers::OpenAICompatibleProvider::new(
                config,
                api_key,
                base_url,
                model,
                max_tokens,
                reasoning,
                reasoning_config,
            ));
        Self {
            provider: provider_impl,
        }
    }

    pub fn from_config(
        api: ApiDriverKind,
        api_key: String,
        base_url: Option<String>,
        model: String,
        max_tokens: u32,
        reasoning: bool,
        reasoning_config: Option<ReasoningConfig>,
        openai_config: Option<OpenAIProviderConfig>,
    ) -> Self {
        if let Some(config) = openai_config {
            Self::with_openai_config(
                config,
                api_key,
                base_url,
                Some(model),
                max_tokens,
                reasoning,
                reasoning_config,
            )
        } else {
            Self::with_provider(
                api,
                api_key,
                base_url,
                Some(model),
                max_tokens,
                reasoning,
                reasoning_config,
            )
        }
    }

    pub async fn stream_message(
        &self,
        system: &[SystemBlock],
        messages: &[Message],
        tool_schemas: &[serde_json::Value],
        handler: &mut dyn StreamHandler,
        cancel: &CancellationToken,
    ) -> Result<StreamResponse, crate::LlmError> {
        self.log_request(system, messages, tool_schemas);
        let result = self
            .provider
            .stream_message(system, messages, tool_schemas, handler, cancel)
            .await;
        self.log_response(&result);
        result
    }

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
        let result = self
            .provider
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
        let msg_summary: Vec<serde_json::Value> = messages.iter().enumerate().map(|(i, msg)| {
            let blocks: Vec<serde_json::Value> = msg.content.iter().map(|block| match block {
                aemeath_core::message::ContentBlock::Text { text } => {
                    serde_json::json!({"type":"text","preview":truncate_preview(text,200)})
                }
                aemeath_core::message::ContentBlock::ToolUse { name, input, .. } => {
                    let input_str = input.to_string();
                    serde_json::json!({"type":"tool_use","name":name,"input_preview":truncate_preview(&input_str,300)})
                }
                aemeath_core::message::ContentBlock::ToolResult { content, is_error, .. } => {
                    let s = content.to_string();
                    serde_json::json!({"type":"tool_result","is_error":is_error,"preview":truncate_preview(&s,300)})
                }
                aemeath_core::message::ContentBlock::Thinking { thinking } => {
                    serde_json::json!({"type":"thinking","preview":truncate_preview(thinking,200)})
                }
                aemeath_core::message::ContentBlock::Image { .. } => {
                    serde_json::json!({"type":"image","preview":"[image data]"})
                }
            }).collect();
            serde_json::json!({"index":i,"role":format!("{:?}",msg.role).to_lowercase(),"blocks":blocks})
        }).collect();
        let system_preview: Vec<String> = system
            .iter()
            .map(|b| truncate_preview(&b.text, 200))
            .collect();
        log::debug!(
            "[LLM REQUEST] provider={} model={} system_blocks={} messages={} tools={}\n  system: {:?}\n  messages: {}",
            self.provider_name(), self.model_name(), system.len(), messages.len(), tool_schemas.len(),
            system_preview, serde_json::to_string_pretty(&msg_summary).unwrap_or_default(),
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
                    serde_json::json!({"id":id,"name":name,"input_preview":truncate_preview(&input_str,300)})
                }).collect();
                log::debug!(
                    "[LLM RESPONSE] stop_reason={:?} input_tokens={} output_tokens={} tool_calls={}\n  text: {}\n  tools: {}",
                    resp.stop_reason, resp.usage.input_tokens, resp.usage.output_tokens,
                    tool_uses.len(), text_preview, serde_json::to_string_pretty(&tools_summary).unwrap_or_default(),
                );
            }
            Err(e) => {
                log::debug!("[LLM RESPONSE ERROR] {}", e);
            }
        }
    }

    pub fn model_name(&self) -> &str {
        self.provider.model_name()
    }
    pub fn provider_name(&self) -> &str {
        self.provider.provider_name()
    }
    pub fn set_reasoning(&self, enabled: bool) {
        self.provider.set_reasoning(enabled);
    }
    pub fn is_reasoning(&self) -> bool {
        self.provider.is_reasoning()
    }
    pub fn set_reasoning_effort(&self, effort: Option<String>) {
        self.provider.set_reasoning_effort(effort);
    }
    pub fn reasoning_effort(&self) -> Option<String> {
        self.provider.reasoning_effort()
    }
}
