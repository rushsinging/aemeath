use serde::{Deserialize, Serialize};

use super::capability::ReasoningLevel;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvocationScope {
    model: String,
    max_tokens: u32,
    requested_reasoning: ReasoningLevel,
    effective_reasoning: ReasoningLevel,
}

impl InvocationScope {
    pub fn new(
        model: impl Into<String>,
        max_tokens: u32,
        requested_reasoning: ReasoningLevel,
        effective_reasoning: ReasoningLevel,
    ) -> Result<Self, crate::LlmError> {
        let model = model.into();
        if model.trim().is_empty() {
            return Err(crate::LlmError::Config(
                "invocation model must not be empty".to_string(),
            ));
        }
        if max_tokens == 0 {
            return Err(crate::LlmError::Config(
                "invocation max_tokens must be greater than zero".to_string(),
            ));
        }
        if effective_reasoning > requested_reasoning {
            return Err(crate::LlmError::Config(
                "effective reasoning must not exceed requested reasoning".to_string(),
            ));
        }
        Ok(Self {
            model,
            max_tokens,
            requested_reasoning,
            effective_reasoning,
        })
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    pub fn max_tokens(&self) -> u32 {
        self.max_tokens
    }

    pub fn requested_reasoning(&self) -> ReasoningLevel {
        self.requested_reasoning
    }

    pub fn effective_reasoning(&self) -> ReasoningLevel {
        self.effective_reasoning
    }
}

#[cfg(test)]
mod invocation_scope_tests {
    use super::*;

    #[test]
    fn invocation_scope_freezes_resolved_values() {
        let scope = InvocationScope::new(
            "claude-sonnet",
            4096,
            ReasoningLevel::High,
            ReasoningLevel::Medium,
        )
        .expect("valid scope");

        assert_eq!(scope.model(), "claude-sonnet");
        assert_eq!(scope.max_tokens(), 4096);
        assert_eq!(scope.requested_reasoning(), ReasoningLevel::High);
        assert_eq!(scope.effective_reasoning(), ReasoningLevel::Medium);
    }

    #[test]
    fn invocation_scope_rejects_zero_max_tokens() {
        assert!(
            InvocationScope::new("claude-sonnet", 0, ReasoningLevel::Off, ReasoningLevel::Off,)
                .is_err()
        );
    }

    #[test]
    fn invocation_scope_rejects_effective_reasoning_above_requested() {
        assert!(InvocationScope::new(
            "claude-sonnet",
            4096,
            ReasoningLevel::Low,
            ReasoningLevel::High,
        )
        .is_err());
    }
}

/// A block within the system prompt, supporting prompt caching via cache_control.
#[derive(Debug, Clone, Serialize)]
pub struct SystemBlock {
    #[serde(rename = "type")]
    pub block_type: String,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_control: Option<CacheControl>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CacheControl {
    #[serde(rename = "type")]
    pub control_type: String,
}

impl SystemBlock {
    /// Create a static block with ephemeral cache control.
    pub fn cached(text: String) -> Self {
        Self {
            block_type: "text".to_string(),
            text,
            cache_control: Some(CacheControl {
                control_type: "ephemeral".to_string(),
            }),
        }
    }

    /// Create a dynamic block without caching.
    pub fn dynamic(text: String) -> Self {
        Self {
            block_type: "text".to_string(),
            text,
            cache_control: None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct CreateMessageRequest {
    pub model: String,
    pub max_tokens: u32,
    #[serde(skip_serializing)]
    pub effort: Option<String>,
    system: Vec<SystemBlock>,
    messages: Vec<serde_json::Value>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<serde_json::Value>,
    stream: bool,
}

impl CreateMessageRequest {
    pub fn new(
        model: String,
        max_tokens: u32,
        effort: Option<String>,
        system: Vec<SystemBlock>,
        messages: Vec<serde_json::Value>,
        tools: Vec<serde_json::Value>,
        stream: bool,
    ) -> Self {
        Self {
            model,
            max_tokens,
            effort,
            system,
            messages,
            tools,
            stream,
        }
    }

    pub fn into_json(self) -> serde_json::Value {
        let mut value = serde_json::to_value(&self).unwrap_or_else(|_| serde_json::json!({}));
        match self.effort.as_deref() {
            None => {
                // No reasoning → thinking disabled
                if let Some(obj) = value.as_object_mut() {
                    obj.insert(
                        "thinking".to_string(),
                        serde_json::json!({"type": "disabled"}),
                    );
                }
            }
            Some(effort) => {
                // Has effort → thinking adaptive + output_config.effort.
                // display:"summarized" 让 Opus 4.7+ 返回 thinking_delta 明文
                // （这些模型 display 默认 omitted，只发 signature_delta）。
                if let Some(obj) = value.as_object_mut() {
                    obj.insert(
                        "thinking".to_string(),
                        serde_json::json!({"type": "adaptive", "display": "summarized"}),
                    );
                    obj.insert(
                        "output_config".to_string(),
                        serde_json::json!({"effort": effort}),
                    );
                }
            }
        }
        value
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    #[serde(alias = "input_tokens")]
    pub input_tokens: u32,
    #[serde(alias = "output_tokens")]
    pub output_tokens: u32,
    /// Tokens served from prompt cache (cost-free or reduced cost).
    /// Parsed from `prompt_tokens_details.cached_tokens` (OpenAI-compatible)
    /// or `usage.cache_read_input_tokens` (Anthropic).
    #[serde(default, alias = "cache_read_input_tokens")]
    pub cached_tokens: Option<u32>,
    /// Tokens written to prompt cache this turn (Anthropic
    /// `cache_creation_input_tokens`). Charged at a premium rate; subsequent
    /// turns read from cache at a steep discount.
    #[serde(default, alias = "cache_creation_input_tokens")]
    pub cache_creation_tokens: Option<u32>,
    /// Tokens consumed by reasoning/thinking within the decoder's compatibility aggregate.
    #[serde(default)]
    pub reasoning_tokens: Option<u32>,
    /// Provider-normalized total tokens for this request.
    ///
    /// OpenAI-compatible adapters prefer reported `total_tokens`, falling back to
    /// `input_tokens + output_tokens` without re-adding cached tokens. Anthropic
    /// adapters normalize `input_tokens + cache_read_input_tokens
    /// + cache_creation_input_tokens + output_tokens`.
    #[serde(default)]
    pub total_tokens: Option<u32>,
}

impl Usage {
    pub fn normalized_total_tokens(&self, additional_input_tokens: u32) -> u32 {
        let _reported_reasoning_tokens = self.reasoning_tokens;
        self.total_tokens.unwrap_or_else(|| {
            self.input_tokens
                .saturating_add(additional_input_tokens)
                .saturating_add(self.output_tokens)
        })
    }

    pub fn finalize_total_tokens(&mut self, additional_input_tokens: u32) {
        self.total_tokens = Some(self.normalized_total_tokens(additional_input_tokens));
    }

    pub fn finalize_anthropic_total_tokens(&mut self) {
        let cache_tokens = self
            .cached_tokens
            .unwrap_or(0)
            .saturating_add(self.cache_creation_tokens.unwrap_or(0));
        self.finalize_total_tokens(cache_tokens);
    }
}

#[cfg(test)]
mod usage_tests {
    use super::Usage;

    #[test]
    fn openai_total_prefers_reported_total_and_does_not_add_cached_tokens() {
        let usage = Usage {
            input_tokens: 100,
            output_tokens: 20,
            cached_tokens: Some(80),
            total_tokens: Some(150),
            ..Usage::default()
        };

        assert_eq!(usage.normalized_total_tokens(0), 150);
    }

    #[test]
    fn openai_total_falls_back_to_input_plus_output() {
        let usage = Usage {
            input_tokens: 100,
            output_tokens: 20,
            cached_tokens: Some(80),
            ..Usage::default()
        };

        assert_eq!(usage.normalized_total_tokens(0), 120);
    }

    #[test]
    fn anthropic_total_includes_cache_read_and_creation_tokens() {
        let usage = Usage {
            input_tokens: 100,
            output_tokens: 20,
            cached_tokens: Some(80),
            cache_creation_tokens: Some(30),
            ..Usage::default()
        };

        assert_eq!(usage.normalized_total_tokens(110), 230);
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum StopReason {
    EndTurn,
    ToolUse,
    MaxTokens,
}

impl StopReason {
    pub fn parse(s: &str) -> Self {
        match s {
            "end_turn" => Self::EndTurn,
            "tool_use" => Self::ToolUse,
            "max_tokens" => Self::MaxTokens,
            _ => Self::EndTurn,
        }
    }
}

#[derive(Debug, Clone)]
pub struct StreamResponse {
    pub assistant_message: share::message::Message,
    pub stop_reason: StopReason,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StreamEvent {
    MessageStart {
        message: MessageStartPayload,
    },
    ContentBlockStart {
        // index 为反序列化所需字段，业务侧暂未读取；收窄可见性后暴露为孤儿，保留以正确解析（refs #61 D3）。
        #[allow(dead_code)]
        index: usize,
        content_block: ContentBlockPayload,
    },
    ContentBlockDelta {
        #[allow(dead_code)]
        index: usize,
        delta: DeltaPayload,
    },
    ContentBlockStop {
        #[allow(dead_code)]
        index: usize,
    },
    MessageDelta {
        delta: MessageDeltaPayload,
        usage: Option<DeltaUsage>,
    },
    MessageStop,
    Ping,
    Error {
        error: ApiError,
    },
}

#[derive(Debug, Deserialize)]
pub struct MessageStartPayload {
    pub usage: Usage,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlockPayload {
    Text {
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
    },
    Thinking {
        #[serde(default)]
        thinking: String,
    },
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DeltaPayload {
    TextDelta {
        text: String,
    },
    InputJsonDelta {
        partial_json: String,
    },
    ThinkingDelta {
        #[serde(default)]
        thinking: String,
    },
    SignatureDelta {
        // signature 为反序列化所需字段，业务侧暂未读取；收窄可见性后暴露为孤儿，保留以正确解析（refs #61 D3）。
        #[serde(default)]
        #[allow(dead_code)]
        signature: String,
    },
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Deserialize)]
pub struct MessageDeltaPayload {
    pub stop_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct DeltaUsage {
    pub output_tokens: u32,
}

#[derive(Debug, Deserialize)]
pub struct ApiError {
    #[serde(rename = "type")]
    pub error_type: String,
    pub message: String,
}
