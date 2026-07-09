//! Anthropic message conversion and non-streaming fallback helpers

use reqwest::header::HeaderMap;
use share::message::{ContentBlock, Message, Role};

use crate::business::types::{
    CreateMessageRequest, StopReason, StreamResponse, SystemBlock, Usage,
};
use crate::core::provider::StreamHandler;

// ---------------------------------------------------------------------------
// Tool schema sanitize — strip internal-only fields (data_schema etc.)
// before sending to the Anthropic Messages API. Only spec-allowed keys
// survive: name, description, input_schema, cache_control, type.
// ---------------------------------------------------------------------------

/// Anthropic Messages API tool spec 允许的字段白名单。
const ANTHROPIC_TOOL_ALLOWED_KEYS: &[&str] = &[
    "name",
    "description",
    "input_schema",
    "cache_control",
    "type",
];

/// 将内部 tool schema（含 `data_schema` 等扩展字段）清洗为 Anthropic
/// Messages API 兼容格式，只保留白名单字段。
pub(crate) fn sanitize_tool_schemas(tool_schemas: &[serde_json::Value]) -> Vec<serde_json::Value> {
    let empty = serde_json::Map::new();
    tool_schemas
        .iter()
        .map(|schema| {
            let obj = schema.as_object().unwrap_or(&empty);
            let filtered: serde_json::Map<String, serde_json::Value> = obj
                .iter()
                .filter(|(k, _)| ANTHROPIC_TOOL_ALLOWED_KEYS.contains(&k.as_str()))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            serde_json::Value::Object(filtered)
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Message conversion — explicitly build Anthropic Messages API JSON from
// internal Message, avoiding serde_json::to_value leaks (metadata,
// placeholder, text etc. are internal-only and must never reach the wire).
// ---------------------------------------------------------------------------

/// 将内部 `Message` 列表转换为 Anthropic Messages API 兼容的 JSON 数组。
///
/// 显式遍历每个 `ContentBlock`，按 API 规范构建 wire format，丢弃所有
/// 内部扩展字段（`metadata`、`Image.placeholder`、`ToolResult.text`）。
/// 与 OpenAI 兼容 driver 的 `convert_messages` 对齐——各 driver 负责自己
/// 的 wire format。
pub(crate) fn convert_messages(messages: &[Message]) -> Vec<serde_json::Value> {
    messages.iter().map(convert_message).collect()
}

fn convert_message(msg: &Message) -> serde_json::Value {
    let role = match msg.role {
        Role::User => "user",
        Role::Assistant => "assistant",
    };
    let content: Vec<serde_json::Value> = msg.content.iter().map(convert_block).collect();
    serde_json::json!({
        "role": role,
        "content": content,
    })
}

fn convert_block(block: &ContentBlock) -> serde_json::Value {
    match block {
        ContentBlock::Text { text } => serde_json::json!({
            "type": "text",
            "text": text,
        }),
        ContentBlock::Image { source, .. } => {
            // 丢弃 placeholder（内部 round-trip 字段，不发给 LLM）
            match source {
                share::message::ImageSource::Base64 { media_type, data } => serde_json::json!({
                    "type": "image",
                    "source": {
                        "type": "base64",
                        "media_type": media_type,
                        "data": data,
                    },
                }),
            }
        }
        ContentBlock::ToolUse { id, name, input } => serde_json::json!({
            "type": "tool_use",
            "id": id,
            "name": name,
            "input": input,
        }),
        ContentBlock::ToolResult {
            tool_use_id,
            content,
            is_error,
            ..
        } => {
            // 丢弃 text（内部 text-first 字段，to_llm_view 已降级到 content）
            serde_json::json!({
                "type": "tool_result",
                "tool_use_id": tool_use_id,
                "content": content,
                "is_error": is_error,
            })
        }
        ContentBlock::Thinking { thinking } => serde_json::json!({
            "type": "thinking",
            "thinking": thinking,
        }),
    }
}

// ---------------------------------------------------------------------------
// ---------------------------------------------------------------------------

/// Handler wrapper that tracks whether any user-visible content was emitted.
/// Used to decide if a non-stream fallback is safe on stream errors — if any
/// text/tool_use was already shown, falling back would duplicate it.
pub(crate) struct TrackingHandler<'a> {
    pub(crate) inner: &'a mut dyn StreamHandler,
    pub(crate) emitted: bool,
}

impl<'a> TrackingHandler<'a> {
    pub(crate) fn new(inner: &'a mut dyn StreamHandler) -> Self {
        Self {
            inner,
            emitted: false,
        }
    }
}

impl<'a> StreamHandler for TrackingHandler<'a> {
    fn on_text(&mut self, text: &str) {
        self.emitted = true;
        self.inner.on_text(text);
    }
    fn on_tool_use_start(&mut self, name: &str, provider_id: Option<&str>, index: usize) {
        self.emitted = true;
        self.inner.on_tool_use_start(name, provider_id, index);
    }
    fn on_error(&mut self, error: &str) {
        self.inner.on_error(error);
    }
    fn on_raw_line(&mut self, line: &str) {
        self.inner.on_raw_line(line);
    }
    fn on_block_complete(&mut self, full_text: &str) {
        self.inner.on_block_complete(full_text);
    }
    fn on_thinking(&mut self, text: &str) {
        self.emitted = true;
        self.inner.on_thinking(text);
    }
}

// ---------------------------------------------------------------------------
// Non-streaming fallback
// ---------------------------------------------------------------------------

/// Parameters needed to build and send an Anthropic API request.
/// Extracted so the non-streaming fallback can live in its own file without
/// needing a reference to `AnthropicProvider`.
pub(crate) struct RequestParams<'a> {
    pub model: String,
    pub max_tokens: u32,
    pub effort: Option<String>,
    pub base_url: String,
    pub headers: HeaderMap,
    pub http: &'a reqwest::Client,
}

/// Send a single non-streaming request and feed the result into `handler`.
pub(crate) async fn send_message_non_stream(
    params: RequestParams<'_>,
    system: &[SystemBlock],
    messages: &[Message],
    tool_schemas: &[serde_json::Value],
    handler: &mut dyn StreamHandler,
) -> Result<StreamResponse, crate::LlmError> {
    let api_messages = convert_messages(messages);

    let request = CreateMessageRequest::new(
        params.model,
        params.max_tokens,
        params.effort,
        system.to_vec(),
        api_messages,
        sanitize_tool_schemas(tool_schemas),
        false,
    );

    let url = format!("{}/v1/messages", params.base_url);
    let response = params
        .http
        .post(&url)
        .headers(params.headers)
        .json(&request.clone().into_json())
        .send()
        .await
        .map_err(|e| {
            let mut msg = format!("{}\n  URL: {}", e, url);
            let mut source: Option<&dyn std::error::Error> = std::error::Error::source(&e);
            let mut depth = 1;
            while let Some(cause) = source {
                msg.push_str(&format!("\n  Cause #{}: {}", depth, cause));
                source = cause.source();
                depth += 1;
            }
            crate::LlmError::Network(msg)
        })?;

    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(crate::LlmError::Api {
            error_type: status.to_string(),
            message: body,
        });
    }

    let body: serde_json::Value = response
        .json()
        .await
        .map_err(|e| crate::LlmError::Stream(e.to_string()))?;

    // Parse the non-streaming response into StreamResponse
    let mut content_blocks = Vec::new();
    if let Some(content) = body.get("content").and_then(|v| v.as_array()) {
        for block in content {
            let block_type = block.get("type").and_then(|v| v.as_str()).unwrap_or("");
            match block_type {
                "text" => {
                    if let Some(text) = block.get("text").and_then(|v| v.as_str()) {
                        handler.on_text(text);
                        handler.on_block_complete(text);
                        content_blocks.push(ContentBlock::Text {
                            text: text.to_string(),
                        });
                    }
                }
                "tool_use" => {
                    let id = block
                        .get("id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let name = block
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let input = block
                        .get("input")
                        .cloned()
                        .unwrap_or_else(|| {
                            // 非流式响应已通过 HTTP 完整接收，input 缺失属 provider 协议异常。
                            // 记 warn 让 silent 变 visible；空对象作为兜底避免整个响应失败。
                            // 真正的"截断"问题在流式路径处理（见 business/stream.rs）。
                            log::warn!(
                                target: "aemeath:agent:provider",
                                "Anthropic 非流式响应 tool_use 块缺少 input 字段（id={}, name={}），使用空对象兜底",
                                id, name,
                            );
                            serde_json::Value::Object(serde_json::Map::new())
                        });
                    let idx = content_blocks.len();
                    handler.on_tool_use_start(&name, Some(&id), idx);
                    content_blocks.push(ContentBlock::ToolUse { id, name, input });
                }
                _ => {}
            }
        }
    }

    let usage = Usage {
        input_tokens: body
            .get("usage")
            .and_then(|u| u.get("input_tokens"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32,
        output_tokens: body
            .get("usage")
            .and_then(|u| u.get("output_tokens"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32,
        cached_tokens: body
            .get("usage")
            .and_then(|u| u.get("cache_read_input_tokens"))
            .and_then(|v| v.as_u64())
            .map(|v| v as u32),
        cache_creation_tokens: body
            .get("usage")
            .and_then(|u| u.get("cache_creation_input_tokens"))
            .and_then(|v| v.as_u64())
            .map(|v| v as u32),
        reasoning_tokens: None, // Anthropic 不返回 reasoning_tokens
        total_tokens: None,     // Anthropic 不返回 total_tokens，由消费侧回退 input+output
    };

    let stop_reason_str = body
        .get("stop_reason")
        .and_then(|v| v.as_str())
        .unwrap_or("end_turn");

    Ok(StreamResponse {
        assistant_message: Message {
            role: Role::Assistant,
            content: content_blocks,
            metadata: None,
        },
        usage,
        stop_reason: StopReason::parse(stop_reason_str),
    })
}

#[cfg(test)]
mod tests {
    use super::{convert_messages, sanitize_tool_schemas};
    use share::message::{
        ContentBlock, ImageSource, Message, MessageMetadata, MessageSource, Role,
    };

    #[test]
    fn strips_data_schema_and_keeps_allowed_fields() {
        let schemas = vec![serde_json::json!({
            "name": "Read",
            "description": "Read a file",
            "input_schema": {"type": "object"},
            "data_schema": {"type": "object"},
            "cache_control": {"type": "ephemeral"}
        })];
        let result = sanitize_tool_schemas(&schemas);
        assert_eq!(result.len(), 1);
        let tool = &result[0];
        assert!(tool.get("name").is_some());
        assert!(tool.get("description").is_some());
        assert!(tool.get("input_schema").is_some());
        assert!(tool.get("cache_control").is_some());
        assert!(
            tool.get("data_schema").is_none(),
            "data_schema must be stripped"
        );
    }

    #[test]
    fn preserves_input_schema_content_intact() {
        let input = serde_json::json!({
            "type": "object",
            "properties": {
                "file_path": {"type": "string"}
            },
            "required": ["file_path"]
        });
        let schemas = vec![serde_json::json!({
            "name": "Read",
            "description": "Read",
            "input_schema": input.clone(),
            "data_schema": {"type": "object"},
        })];
        let result = sanitize_tool_schemas(&schemas);
        assert_eq!(result[0].get("input_schema").unwrap(), &input);
    }

    #[test]
    fn handles_empty_schemas() {
        let result = sanitize_tool_schemas(&[]);
        assert!(result.is_empty());
    }

    // --- convert_messages tests ---

    #[test]
    fn convert_messages_strips_metadata() {
        let msg = Message {
            role: Role::User,
            content: vec![ContentBlock::Text {
                text: "hi".to_string(),
            }],
            metadata: Some(MessageMetadata {
                source: MessageSource::SystemGenerated,
            }),
        };
        let result = convert_messages(&[msg]);
        assert_eq!(result.len(), 1);
        assert!(
            result[0].get("metadata").is_none(),
            "metadata must be stripped"
        );
        assert_eq!(result[0]["role"], "user");
    }

    #[test]
    fn convert_messages_text_block() {
        let msg = Message::user("hello world");
        let result = convert_messages(&[msg]);
        let block = &result[0]["content"][0];
        assert_eq!(block["type"], "text");
        assert_eq!(block["text"], "hello world");
    }

    #[test]
    fn convert_messages_image_strips_placeholder() {
        let msg = Message {
            role: Role::User,
            content: vec![ContentBlock::Image {
                source: ImageSource::Base64 {
                    media_type: "image/png".to_string(),
                    data: "abc123".to_string(),
                },
                placeholder: Some("[Image #1]".to_string()),
            }],
            metadata: None,
        };
        let result = convert_messages(&[msg]);
        let block = &result[0]["content"][0];
        assert_eq!(block["type"], "image");
        assert_eq!(block["source"]["type"], "base64");
        assert_eq!(block["source"]["media_type"], "image/png");
        assert_eq!(block["source"]["data"], "abc123");
        assert!(
            block.get("placeholder").is_none(),
            "placeholder must be stripped"
        );
    }

    #[test]
    fn convert_messages_tool_use() {
        let msg = Message {
            role: Role::Assistant,
            content: vec![ContentBlock::ToolUse {
                id: "tu_1".to_string(),
                name: "Read".to_string(),
                input: serde_json::json!({"file_path": "/tmp/a"}),
            }],
            metadata: None,
        };
        let result = convert_messages(&[msg]);
        let block = &result[0]["content"][0];
        assert_eq!(block["type"], "tool_use");
        assert_eq!(block["id"], "tu_1");
        assert_eq!(block["name"], "Read");
        assert_eq!(block["input"]["file_path"], "/tmp/a");
    }

    #[test]
    fn convert_messages_tool_result_strips_text_field() {
        let msg = Message {
            role: Role::User,
            content: vec![ContentBlock::ToolResult {
                tool_use_id: "tu_1".to_string(),
                content: serde_json::json!("done"),
                is_error: false,
                text: Some("done".to_string()),
            }],
            metadata: None,
        };
        let result = convert_messages(&[msg]);
        let block = &result[0]["content"][0];
        assert_eq!(block["type"], "tool_result");
        assert_eq!(block["tool_use_id"], "tu_1");
        assert_eq!(block["content"], "done");
        assert_eq!(block["is_error"], false);
        assert!(block.get("text").is_none(), "text field must be stripped");
    }

    #[test]
    fn convert_messages_thinking_block() {
        let msg = Message {
            role: Role::Assistant,
            content: vec![ContentBlock::Thinking {
                thinking: "let me think".to_string(),
            }],
            metadata: None,
        };
        let result = convert_messages(&[msg]);
        let block = &result[0]["content"][0];
        assert_eq!(block["type"], "thinking");
        assert_eq!(block["thinking"], "let me think");
    }

    #[test]
    fn convert_messages_assistant_role() {
        let msg = Message {
            role: Role::Assistant,
            content: vec![ContentBlock::Text {
                text: "ok".to_string(),
            }],
            metadata: None,
        };
        let result = convert_messages(&[msg]);
        assert_eq!(result[0]["role"], "assistant");
    }
}
