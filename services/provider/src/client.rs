//! Unified LLM client that supports multiple providers

use std::error::Error as StdError;
use std::sync::Arc;

use crate::api::ApiDriverKind;
use crate::provider::{CallbackHandler, LlmProvider, StreamHandler};
use crate::providers::openai_compatible::ReasoningConfig;
use crate::types::{StreamResponse, SystemBlock};
use aemeath_core::message::Message;
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

fn llm_error_chain(error: &crate::LlmError) -> String {
    let mut chain = String::new();
    let mut source = StdError::source(error);
    let mut depth = 1;
    while let Some(cause) = source {
        chain.push_str(&format!("\n  Cause #{}: {}", depth, cause));
        source = cause.source();
        depth += 1;
    }
    chain
}

fn messages_payload_bytes(messages: &[Message]) -> usize {
    serde_json::to_string(messages)
        .map(|s| s.len())
        .unwrap_or(0)
}

fn content_block_counts(messages: &[Message]) -> (usize, usize, usize, usize, usize) {
    let mut text = 0;
    let mut thinking = 0;
    let mut tool_use = 0;
    let mut tool_result = 0;
    let mut image = 0;
    for msg in messages {
        for block in &msg.content {
            match block {
                aemeath_core::message::ContentBlock::Text { .. } => text += 1,
                aemeath_core::message::ContentBlock::Thinking { .. } => thinking += 1,
                aemeath_core::message::ContentBlock::ToolUse { .. } => tool_use += 1,
                aemeath_core::message::ContentBlock::ToolResult { .. } => tool_result += 1,
                aemeath_core::message::ContentBlock::Image { .. } => image += 1,
            }
        }
    }
    (text, thinking, tool_use, tool_result, image)
}

fn largest_message_summary(messages: &[Message]) -> (usize, String, usize) {
    messages
        .iter()
        .enumerate()
        .map(|(idx, msg)| {
            let bytes = serde_json::to_string(msg).map(|s| s.len()).unwrap_or(0);
            (idx, format!("{:?}", msg.role).to_lowercase(), bytes)
        })
        .max_by_key(|(_, _, bytes)| *bytes)
        .unwrap_or((0, "none".to_string(), 0))
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
                ApiDriverKind::Volcengine => "/chat/completions".to_string(),
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
    pub fn from_provider(provider: Arc<dyn LlmProvider>) -> Self {
        Self { provider }
    }
}

impl LlmClient {
    pub fn new(api_key: String) -> Self {
        Self::with_provider(
            ApiDriverKind::Anthropic,
            api_key,
            None,
            None,
            200000,
            0,
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
        thinking_max_tokens: u32,
        reasoning: bool,
        reasoning_config: Option<ReasoningConfig>,
    ) -> Self {
        let provider_impl: Arc<dyn LlmProvider> = match api {
            ApiDriverKind::Anthropic => Arc::new(crate::providers::AnthropicProvider::new(
                api_key,
                base_url,
                model,
                max_tokens,
                thinking_max_tokens,
            )),
            ApiDriverKind::OpenAI
            | ApiDriverKind::Zhipu
            | ApiDriverKind::LiteLLM
            | ApiDriverKind::Volcengine => {
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
        thinking_max_tokens: u32,
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
                thinking_max_tokens,
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
        if let Err(error) = &result {
            self.log_stream_error("stream_message", system, messages, tool_schemas, error);
        }
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
        if let Err(error) = &result {
            self.log_stream_error("stream_message_raw", system, messages, tool_schemas, error);
        }
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
                log::warn!("[LLM RESPONSE ERROR] {}{}", e, llm_error_chain(e));
            }
        }
    }

    fn log_stream_error(
        &self,
        phase: &str,
        system: &[SystemBlock],
        messages: &[Message],
        tool_schemas: &[serde_json::Value],
        error: &crate::LlmError,
    ) {
        let (text_blocks, thinking_blocks, tool_use_blocks, tool_result_blocks, image_blocks) =
            content_block_counts(messages);
        let (largest_idx, largest_role, largest_bytes) = largest_message_summary(messages);
        log::warn!(
            "[LLM STREAM ERROR] phase={} provider={} model={} system_blocks={} messages={} tools={} messages_payload_bytes={} content_blocks={{text:{},thinking:{},tool_use:{},tool_result:{},image:{}}} largest_message={{index:{},role:{},bytes:{}}} error={}{}",
            phase,
            self.provider_name(),
            self.model_name(),
            system.len(),
            messages.len(),
            tool_schemas.len(),
            messages_payload_bytes(messages),
            text_blocks,
            thinking_blocks,
            tool_use_blocks,
            tool_result_blocks,
            image_blocks,
            largest_idx,
            largest_role,
            largest_bytes,
            error,
            llm_error_chain(error),
        );
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
    pub fn set_max_tokens(&self, max_tokens: u32) {
        self.provider.set_max_tokens(max_tokens);
    }
    pub fn max_tokens(&self) -> u32 {
        self.provider.max_tokens()
    }
}
