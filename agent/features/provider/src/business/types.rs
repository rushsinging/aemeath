use serde::{Deserialize, Serialize};

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
    #[serde(skip_serializing_if = "is_zero")]
    pub thinking_max_tokens: u32,
    system: Vec<SystemBlock>,
    messages: Vec<serde_json::Value>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<serde_json::Value>,
    stream: bool,
}

fn is_zero(value: &u32) -> bool {
    *value == 0
}

impl CreateMessageRequest {
    pub fn new(
        model: String,
        max_tokens: u32,
        thinking_max_tokens: u32,
        system: Vec<SystemBlock>,
        messages: Vec<serde_json::Value>,
        tools: Vec<serde_json::Value>,
        stream: bool,
    ) -> Self {
        Self {
            model,
            max_tokens,
            thinking_max_tokens,
            system,
            messages,
            tools,
            stream,
        }
    }

    pub fn into_json(self) -> serde_json::Value {
        let mut value = serde_json::to_value(self).unwrap_or_else(|_| serde_json::json!({}));
        if let Some(tokens) = value
            .get("thinking_max_tokens")
            .and_then(|v| v.as_u64())
            .filter(|tokens| *tokens > 0)
        {
            if let Some(obj) = value.as_object_mut() {
                obj.remove("thinking_max_tokens");
                obj.insert(
                    "thinking".to_string(),
                    serde_json::json!({"type": "enabled", "budget_tokens": tokens}),
                );
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
    /// Tokens consumed by reasoning/thinking.
    /// Parsed from `completion_tokens_details.reasoning_tokens` (OpenAI-compatible)
    /// or `usage.reasoning_tokens` (if provider supports it).
    #[serde(default)]
    pub reasoning_tokens: Option<u32>,
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
    pub usage: Usage,
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
