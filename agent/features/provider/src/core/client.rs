//! Unified LLM client that supports multiple providers

use std::error::Error as StdError;
use std::sync::Arc;

use crate::api::ProviderDriverKind;
use crate::business::providers::openai_compatible::ReasoningConfig;
use crate::business::types::{StreamResponse, SystemBlock};
use crate::core::provider::{CallbackHandler, LlmProvider, StreamHandler};
use crate::LOG_TARGET;
use share::message::Message;
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
                share::message::ContentBlock::Text { .. } => text += 1,
                share::message::ContentBlock::Thinking { .. } => thinking += 1,
                share::message::ContentBlock::ToolUse { .. } => tool_use += 1,
                share::message::ContentBlock::ToolResult { .. } => tool_result += 1,
                share::message::ContentBlock::Image { .. } => image += 1,
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
/// for display/logging; API behavior comes from `driver`.
#[derive(Debug, Clone)]
pub struct OpenAIProviderConfig {
    pub source_key: String,
    pub driver: ProviderDriverKind,
    pub chat_api_suffix: String,
}

impl OpenAIProviderConfig {
    pub fn from_driver(driver: ProviderDriverKind, source_key: &str) -> Self {
        Self {
            source_key: source_key.to_string(),
            driver,
            chat_api_suffix: match driver {
                ProviderDriverKind::Zhipu => "/chat/completions".to_string(),
                ProviderDriverKind::Anthropic => "/v1/messages".to_string(),
                ProviderDriverKind::Volcengine => "/chat/completions".to_string(),
                ProviderDriverKind::Minimax => "/chat/completions".to_string(),
                ProviderDriverKind::Mimo => "/chat/completions".to_string(),
                ProviderDriverKind::DeepSeek => "/chat/completions".to_string(),
                ProviderDriverKind::Agnes => "/chat/completions".to_string(),
                // Ollama 有专用 OllamaProvider，不经此 OpenAI 兼容路径；
                // 兜底归入 OpenAI 风格 suffix。
                ProviderDriverKind::OpenAI
                | ProviderDriverKind::LiteLLM
                | ProviderDriverKind::Ollama => "/v1/chat/completions".to_string(),
            },
        }
    }
}

pub struct LlmProviderOptions {
    pub driver: ProviderDriverKind,
    pub api_key: String,
    pub base_url: Option<String>,
    pub model: Option<String>,
    pub max_tokens: u32,
    pub reasoning: bool,
    pub reasoning_config: Option<ReasoningConfig>,
}

pub struct LlmConfigOptions {
    pub driver: ProviderDriverKind,
    pub api_key: String,
    pub base_url: Option<String>,
    pub model: String,
    pub max_tokens: u32,
    pub reasoning: bool,
    pub reasoning_config: Option<ReasoningConfig>,
    pub openai_config: Option<OpenAIProviderConfig>,
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
        Self::with_provider(LlmProviderOptions {
            driver: ProviderDriverKind::Anthropic,
            api_key,
            base_url: None,
            model: None,
            max_tokens: 200000,
            reasoning: false,
            reasoning_config: None,
        })
    }

    pub fn with_provider(options: LlmProviderOptions) -> Self {
        let provider_impl: Arc<dyn LlmProvider> = match options.driver {
            ProviderDriverKind::Anthropic => {
                Arc::new(crate::business::providers::AnthropicProvider::new(
                    options.api_key,
                    options.base_url,
                    options.model,
                    options.max_tokens,
                    crate::core::provider::ReasoningLevel::Off,
                ))
            }
            ProviderDriverKind::Ollama => {
                Arc::new(crate::business::providers::OllamaProvider::new(
                    options.api_key,
                    options.base_url,
                    options.model,
                    options.max_tokens,
                    options.reasoning,
                ))
            }
            ProviderDriverKind::OpenAI
            | ProviderDriverKind::Zhipu
            | ProviderDriverKind::LiteLLM
            | ProviderDriverKind::Volcengine
            | ProviderDriverKind::Minimax
            | ProviderDriverKind::Mimo
            | ProviderDriverKind::DeepSeek
            | ProviderDriverKind::Agnes => {
                let config =
                    OpenAIProviderConfig::from_driver(options.driver, options.driver.as_str());
                Arc::new(crate::business::providers::OpenAICompatibleProvider::new(
                    config,
                    options.api_key,
                    options.base_url,
                    options.model,
                    options.max_tokens,
                    options.reasoning,
                    options.reasoning_config,
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
            Arc::new(crate::business::providers::OpenAICompatibleProvider::new(
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

    pub fn from_config(options: LlmConfigOptions) -> Self {
        if let Some(config) = options.openai_config {
            Self::with_openai_config(
                config,
                options.api_key,
                options.base_url,
                Some(options.model),
                options.max_tokens,
                options.reasoning,
                options.reasoning_config,
            )
        } else {
            Self::with_provider(LlmProviderOptions {
                driver: options.driver,
                api_key: options.api_key,
                base_url: options.base_url,
                model: Some(options.model),
                max_tokens: options.max_tokens,
                reasoning: options.reasoning,
                reasoning_config: options.reasoning_config,
            })
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
                share::message::ContentBlock::Text { text } => {
                    serde_json::json!({"type":"text","preview":truncate_preview(text,200)})
                }
                share::message::ContentBlock::ToolUse { name, input, .. } => {
                    let input_str = input.to_string();
                    serde_json::json!({"type":"tool_use","name":name,"input_preview":truncate_preview(&input_str,300)})
                }
                share::message::ContentBlock::ToolResult { content, is_error, .. } => {
                    let s = content.to_string();
                    serde_json::json!({"type":"tool_result","is_error":is_error,"preview":truncate_preview(&s,300)})
                }
                share::message::ContentBlock::Thinking { thinking } => {
                    serde_json::json!({"type":"thinking","preview":truncate_preview(thinking,200)})
                }
                share::message::ContentBlock::Image { .. } => {
                    serde_json::json!({"type":"image","preview":"[image data]"})
                }
            }).collect();
            serde_json::json!({"index":i,"role":format!("{:?}",msg.role).to_lowercase(),"blocks":blocks})
        }).collect();
        let system_preview: Vec<String> = system
            .iter()
            .map(|b| truncate_preview(&b.text, 200))
            .collect();
        // 计算 messages 总字符数用于 DEBUG 摘要
        let total_chars: usize = messages
            .iter()
            .flat_map(|m| m.content.iter())
            .map(|b| match b {
                share::message::ContentBlock::Text { text } => text.len(),
                share::message::ContentBlock::Thinking { thinking } => thinking.len(),
                share::message::ContentBlock::ToolUse { input, .. } => input.to_string().len(),
                share::message::ContentBlock::ToolResult { content, .. } => {
                    content.to_string().len()
                }
                share::message::ContentBlock::Image { .. } => 0,
            })
            .sum();
        log::debug!(target: LOG_TARGET,
            "[LLM REQUEST] provider={} model={} system_blocks={} messages={}({} chars) tools={}",
            self.provider_name(), self.model_name(), system.len(), messages.len(), total_chars, tool_schemas.len(),
        );
        log::trace!(target: LOG_TARGET,
            "[LLM REQUEST] system: {:?}\n  messages: {}",
            system_preview, serde_json::to_string_pretty(&msg_summary).unwrap_or_default(),
        );
    }

    fn log_response(&self, result: &Result<StreamResponse, crate::LlmError>) {
        if !log::log_enabled!(log::Level::Debug) {
            return;
        }
        if let Ok(resp) = result {
            let text = resp.assistant_message.text_content();
            let text_preview = truncate_preview(&text, 500);
            let tool_uses = resp.assistant_message.extract_tool_uses();
            let tools_summary: Vec<serde_json::Value> = tool_uses.iter().map(|(id, name, input)| {
                let input_str = input.to_string();
                serde_json::json!({"id":id,"name":name,"input_preview":truncate_preview(&input_str,300)})
            }).collect();

            // 提取 thinking 块诊断信息
            let thinking_info: Vec<serde_json::Value> = resp
                .assistant_message
                .content
                .iter()
                .filter_map(|block| {
                    if let share::message::ContentBlock::Thinking { thinking } = block {
                        let lines: Vec<&str> = thinking.lines().collect();
                        let mut dup_lines = 0;
                        for i in 1..lines.len() {
                            if lines[i] == lines[i - 1] && !lines[i].trim().is_empty() {
                                dup_lines += 1;
                            }
                        }
                        Some(serde_json::json!({
                            "thinking_len": thinking.len(),
                            "thinking_chars": thinking.chars().count(),
                            "dup_lines": dup_lines,
                            "preview": truncate_preview(thinking, 200),
                        }))
                    } else {
                        None
                    }
                })
                .collect();

            log::debug!(target: LOG_TARGET,
                "[LLM RESPONSE] stop_reason={:?} input_tokens={} output_tokens={} tool_calls={} thinking_blocks={}",
                resp.stop_reason, resp.usage.input_tokens, resp.usage.output_tokens,
                tool_uses.len(), thinking_info.len(),
            );
            log::trace!(target: LOG_TARGET,
                "[LLM RESPONSE] text: {}\n  thinking: {}\n  tools: {}",
                text_preview,
                serde_json::to_string_pretty(&thinking_info).unwrap_or_default(),
                serde_json::to_string_pretty(&tools_summary).unwrap_or_default(),
            );
        }
        // Error path already handled by log_stream_error() with comprehensive context;
        // no need to log a redundant warn here.
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
        log::warn!(target: LOG_TARGET,
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
    pub fn set_reasoning_level(&self, level: crate::core::provider::ReasoningLevel) {
        self.provider.set_reasoning_level(level);
    }
    pub fn current_reasoning_level(&self) -> crate::core::provider::ReasoningLevel {
        self.provider.current_reasoning_level()
    }
    pub fn max_reasoning_level(&self) -> crate::core::provider::ReasoningLevel {
        self.provider.max_reasoning_level()
    }
    pub fn is_reasoning(&self) -> bool {
        self.provider.is_reasoning()
    }
    pub fn set_max_tokens(&self, max_tokens: u32) {
        self.provider.set_max_tokens(max_tokens);
    }
    pub fn max_tokens(&self) -> u32 {
        self.provider.max_tokens()
    }
}
