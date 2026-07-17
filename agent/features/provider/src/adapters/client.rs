//! Unified LLM client that supports multiple providers

use std::sync::Arc;

use crate::adapters::openai_compatible::ReasoningConfig;
use crate::domain::invoke::SystemBlock;
use crate::ports::LlmProvider;
use crate::ProviderDriverKind;
use crate::LOG_TARGET;
use share::message::Message;
use tokio_util::sync::CancellationToken;

fn reasoning_level_from_options(
    reasoning: bool,
    config: Option<&ReasoningConfig>,
) -> crate::ReasoningLevel {
    match config {
        Some(ReasoningConfig::Object(value)) => value
            .get("effort")
            .or_else(|| value.get("reasoning_effort"))
            .and_then(|value| value.as_str())
            .and_then(crate::ReasoningLevel::parse)
            .unwrap_or(if reasoning {
                crate::ReasoningLevel::High
            } else {
                crate::ReasoningLevel::Off
            }),
        Some(ReasoningConfig::ThinkingBudget(tokens)) => match *tokens {
            0 => crate::ReasoningLevel::Off,
            1..=1024 => crate::ReasoningLevel::Low,
            1025..=8192 => crate::ReasoningLevel::Medium,
            8193..=32768 => crate::ReasoningLevel::High,
            _ => crate::ReasoningLevel::Xhigh,
        },
        Some(ReasoningConfig::Bool(enabled)) => {
            if *enabled {
                crate::ReasoningLevel::High
            } else {
                crate::ReasoningLevel::Off
            }
        }
        None if reasoning => crate::ReasoningLevel::High,
        None => crate::ReasoningLevel::Off,
    }
}

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
/// for display/logging; API behavior comes from `driver`.
#[derive(Debug, Clone)]
pub(crate) struct OpenAIProviderConfig {
    pub source_key: String,
    pub driver: ProviderDriverKind,
    pub chat_api_suffix: String,
    /// 是否使用 Responses API（/v1/responses）替代 Chat Completions。
    pub use_responses_api: bool,
}

impl OpenAIProviderConfig {
    pub(crate) fn from_driver(driver: ProviderDriverKind, source_key: &str) -> Self {
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
            use_responses_api: false,
        }
    }

    pub(crate) fn with_responses_api(mut self, enabled: bool) -> Self {
        self.use_responses_api = enabled;
        self
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
    pub timeout_secs: u64,
}

pub struct LlmConfigOptions {
    pub driver: String,
    pub source_key: String,
    pub api_style: Option<String>,
    pub api_key: String,
    pub base_url: Option<String>,
    pub model: String,
    pub max_tokens: u32,
    pub reasoning: bool,
    pub reasoning_config: Option<ReasoningConfig>,
    pub timeout_secs: u64,
}

pub struct LlmClient {
    provider: Arc<dyn LlmProvider>,
    default_scope: crate::InvocationScope,
}

impl LlmClient {
    pub fn from_provider(provider: Arc<dyn LlmProvider>) -> Self {
        let default_scope = crate::InvocationScope::new(
            provider.model_name(),
            share::config::models::DEFAULT_MAX_TOKENS,
            crate::ReasoningLevel::Off,
            crate::ReasoningLevel::Off,
        )
        .expect("provider defaults must form a valid invocation scope");
        Self {
            provider,
            default_scope,
        }
    }
}

impl LlmClient {
    pub fn new(api_key: String) -> Self {
        Self::with_provider(LlmProviderOptions {
            driver: ProviderDriverKind::Anthropic,
            api_key,
            base_url: None,
            model: None,
            max_tokens: 8192,
            reasoning: false,
            reasoning_config: None,
            timeout_secs: crate::DEFAULT_TIMEOUT_SECS,
        })
    }

    pub fn with_provider(options: LlmProviderOptions) -> Self {
        let model = options.model.clone();
        let requested_reasoning =
            reasoning_level_from_options(options.reasoning, options.reasoning_config.as_ref());
        let provider_impl: Arc<dyn LlmProvider> = match options.driver {
            ProviderDriverKind::Anthropic => Arc::new(crate::adapters::AnthropicProvider::new(
                options.api_key,
                options.base_url,
                options.model,
                options.max_tokens,
                crate::ports::ReasoningLevel::Off,
                options.timeout_secs,
            )),
            ProviderDriverKind::Ollama => Arc::new(crate::adapters::OllamaProvider::new(
                options.api_key,
                options.base_url,
                options.model,
                options.max_tokens,
                options.reasoning,
                options.timeout_secs,
            )),
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
                Arc::new(crate::adapters::OpenAICompatibleProvider::new(
                    config,
                    options.api_key,
                    options.base_url,
                    options.model,
                    options.max_tokens,
                    options.reasoning,
                    options.reasoning_config,
                    options.timeout_secs,
                ))
            }
        };
        let effective_reasoning =
            requested_reasoning.clamped_to(provider_impl.max_reasoning_level());
        let default_scope = crate::InvocationScope::new(
            model.unwrap_or_else(|| provider_impl.model_name().to_string()),
            if options.max_tokens == 0 {
                share::config::models::DEFAULT_MAX_TOKENS
            } else {
                options.max_tokens
            },
            requested_reasoning,
            effective_reasoning,
        )
        .expect("provider options must form a valid invocation scope");
        Self {
            provider: provider_impl,
            default_scope,
        }
    }

    pub fn from_config(options: LlmConfigOptions) -> Result<Self, crate::LlmError> {
        use crate::domain::driver_acl::{ApiStyle, DriverSpec, ProtocolFamily};

        let spec = DriverSpec::parse(&options.driver, options.api_style.as_deref())
            .map_err(|error| crate::LlmError::Config(error.to_string()))?;
        let driver = spec.kind();
        let requested_reasoning =
            reasoning_level_from_options(options.reasoning, options.reasoning_config.as_ref());
        let model = options.model.clone();
        let provider_impl: Arc<dyn LlmProvider> = match spec.family() {
            ProtocolFamily::AnthropicMessages => Arc::new(crate::adapters::AnthropicProvider::new(
                options.api_key,
                options.base_url,
                Some(options.model),
                options.max_tokens,
                crate::ports::ReasoningLevel::Off,
                options.timeout_secs,
            )),
            ProtocolFamily::OllamaNative => Arc::new(crate::adapters::OllamaProvider::new(
                options.api_key,
                options.base_url,
                Some(options.model),
                options.max_tokens,
                options.reasoning,
                options.timeout_secs,
            )),
            ProtocolFamily::OpenAi(api_style) => {
                let config = OpenAIProviderConfig::from_driver(driver, &options.source_key)
                    .with_responses_api(api_style == ApiStyle::Responses);
                Arc::new(crate::adapters::OpenAICompatibleProvider::new(
                    config,
                    options.api_key,
                    options.base_url,
                    Some(options.model),
                    options.max_tokens,
                    options.reasoning,
                    options.reasoning_config,
                    options.timeout_secs,
                ))
            }
        };
        let effective_reasoning =
            requested_reasoning.clamped_to(provider_impl.max_reasoning_level());
        let default_scope = crate::InvocationScope::new(
            model,
            if options.max_tokens == 0 {
                share::config::models::DEFAULT_MAX_TOKENS
            } else {
                options.max_tokens
            },
            requested_reasoning,
            effective_reasoning,
        )?;
        Ok(Self {
            provider: provider_impl,
            default_scope,
        })
    }

    pub async fn invocation_stream(
        &self,
        scope: &crate::InvocationScope,
        system: &[SystemBlock],
        messages: &[Message],
        tool_schemas: &[serde_json::Value],
        cancel: &CancellationToken,
    ) -> Result<crate::InvocationStream, crate::ProviderError> {
        self.log_request(system, messages, tool_schemas);
        self.provider
            .invocation_stream(scope, system, messages, tool_schemas, cancel)
            .await
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
                share::message::ContentBlock::Thinking { thinking, .. } => {
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
                share::message::ContentBlock::Thinking { thinking, .. } => thinking.len(),
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

    pub fn with_default_reasoning(
        mut self,
        requested_reasoning: crate::ReasoningLevel,
    ) -> Result<Self, crate::LlmError> {
        self.default_scope = crate::InvocationScope::new(
            self.default_scope.model(),
            self.default_scope.max_tokens(),
            requested_reasoning,
            requested_reasoning.clamped_to(self.provider.max_reasoning_level()),
        )?;
        Ok(self)
    }

    pub fn default_scope(&self) -> &crate::InvocationScope {
        &self.default_scope
    }

    pub fn invocation_scope(
        &self,
        model: impl Into<String>,
        max_tokens: Option<u32>,
        requested_reasoning: crate::ReasoningLevel,
    ) -> Result<crate::InvocationScope, crate::LlmError> {
        crate::InvocationScope::new(
            model,
            max_tokens.unwrap_or_else(|| self.default_scope.max_tokens()),
            requested_reasoning,
            requested_reasoning.clamped_to(self.provider.max_reasoning_level()),
        )
    }

    pub fn model_name(&self) -> &str {
        self.provider.model_name()
    }
    pub fn provider_name(&self) -> &str {
        self.provider.provider_name()
    }
    pub fn max_reasoning_level(&self) -> crate::ports::ReasoningLevel {
        self.provider.max_reasoning_level()
    }
}
