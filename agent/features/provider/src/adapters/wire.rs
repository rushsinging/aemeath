//! Driver 共享 wire 契约（Anthropic 兼容形态）。
//!
//! HTTP/SSE wire DTO（请求/事件/用量/错误载荷）原位于 domain/invoke.rs，
//! 按 #1861 C7 下移 adapters——domain 只留调用领域 VO。openai/ollama
//! driver 复用该兼容形态（历史判例），物理归属随 #1831 后续 BC 化再裁定。

use serde::{Deserialize, Serialize};

/// A block within the system prompt, supporting prompt caching via cache_control.
#[derive(Debug, Clone, Serialize)]
pub struct SystemBlockData {
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

impl SystemBlockData {
    /// 整段 system prompt → wire 块序列（#1861 v4：上游拼好整串，
    /// 可缓存前缀分界由 `static_prefix_len` 字节位置给出）。
    ///
    /// - 空 prompt → 空序列（保持现状 `system: []`）；
    /// - `static_prefix_len == 0` → 整段一个无 cache 标记的块；
    /// - 否则前缀段（`[..static_prefix_len]`）带 `cache_control: ephemeral`、
    ///   余下段无标记（空则省略）。分界天然落在原块边界上，char-safe；
    ///   越界/非字符边界按 0 处理（防御，不 panic）。
    pub fn from_prompt(system: &str, static_prefix_len: usize) -> Vec<SystemBlockData> {
        if system.is_empty() {
            return Vec::new();
        }
        let split_ok = static_prefix_len > 0
            && static_prefix_len <= system.len()
            && system.is_char_boundary(static_prefix_len);
        if !split_ok {
            return vec![SystemBlockData::dynamic(system.to_string())];
        }
        // 安全切分（no-unsafe-text-slicing）：按字节游标逐字符切两段。
        let mut byte_cursor = 0usize;
        let mut char_count = 0usize;
        for (offset, _) in system.char_indices() {
            if offset >= static_prefix_len {
                break;
            }
            byte_cursor = offset + system[offset..].chars().next().map_or(1, |c| c.len_utf8());
            char_count += 1;
        }
        let cached: String = system.chars().take(char_count).collect();
        let rest: String = system.chars().skip(char_count).collect();
        debug_assert_eq!(cached.len(), byte_cursor);
        let mut blocks = vec![SystemBlockData::cached(cached)];
        if !rest.is_empty() {
            blocks.push(SystemBlockData::dynamic(rest));
        }
        blocks
    }

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
    system: Vec<SystemBlockData>,
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
        system: Vec<SystemBlockData>,
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
    /// Tokens written to prompt cache this run (Anthropic
    /// `cache_creation_input_tokens`). Charged at a premium rate; subsequent
    /// runs read from cache at a steep discount.
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

/// #1861 v4：整段 system + `static_prefix_len` 分界的 cache 切分契约。
#[cfg(test)]
mod prompt_cache_split_tests {
    use super::SystemBlockData;

    #[test]
    fn prefix_block_carries_ephemeral_and_suffix_block_has_no_marker() {
        let system = "static prefix\n\ndynamic suffix";
        // 分界落在最后一个可缓存块末尾（连接符归后缀段）——llm_strategy 的
        // static_prefix_len 语义。
        let cut = "static prefix".len();
        let blocks = SystemBlockData::from_prompt(system, cut);

        assert_eq!(blocks.len(), 2, "两段 join：前缀段 + 后缀段");
        assert_eq!(blocks[0].text, "static prefix");
        assert_eq!(
            blocks[0]
                .cache_control
                .as_ref()
                .map(|control| control.control_type.as_str()),
            Some("ephemeral"),
            "前缀段必须带 cache_control: ephemeral"
        );
        assert_eq!(blocks[1].text, "\n\ndynamic suffix");
        assert!(blocks[1].cache_control.is_none(), "后缀段必须无 cache 标记");
    }

    #[test]
    fn zero_prefix_marks_whole_prompt_unmarked() {
        let blocks = SystemBlockData::from_prompt("entire prompt", 0);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].text, "entire prompt");
        assert!(blocks[0].cache_control.is_none());
    }

    #[test]
    fn full_length_prefix_yields_single_cached_block() {
        let blocks = SystemBlockData::from_prompt("fully cacheable", 15);
        assert_eq!(blocks.len(), 1);
        assert_eq!(
            blocks[0]
                .cache_control
                .as_ref()
                .map(|control| control.control_type.as_str()),
            Some("ephemeral")
        );
    }

    #[test]
    fn empty_prompt_yields_no_blocks() {
        assert!(SystemBlockData::from_prompt("", 0).is_empty());
    }

    #[test]
    fn non_char_boundary_cut_falls_back_to_unmarked_whole_prompt() {
        // "é" 2 字节——cut=1 落在字符中间，必须回退而非 panic。
        let blocks = SystemBlockData::from_prompt("é prompt", 1);
        assert_eq!(blocks.len(), 1);
        assert!(blocks[0].cache_control.is_none());
        assert_eq!(blocks[0].text, "é prompt");
    }
}

#[derive(Debug, Clone)]
pub struct StreamResponse {
    pub assistant_message: share::message::Message,
    pub stop_reason: crate::published_language::StopReason,
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
