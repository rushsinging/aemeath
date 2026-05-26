//! Anthropic message conversion and non-streaming fallback helpers

use reqwest::header::HeaderMap;
use share::message::{ContentBlock, Message, Role};

use crate::provider::StreamHandler;
use crate::types::{CreateMessageRequest, StopReason, StreamResponse, SystemBlock, Usage};

// ---------------------------------------------------------------------------
// TrackingHandler – wraps a StreamHandler to detect if any user-visible
// content (text / tool_use / thinking) was emitted.
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
    fn on_tool_use_start(&mut self, name: &str, index: usize) {
        self.emitted = true;
        self.inner.on_tool_use_start(name, index);
    }
    fn on_error(&mut self, error: &str) {
        self.inner.on_error(error);
    }
    fn on_raw_line(&mut self, line: &str) {
        self.inner.on_raw_line(line);
    }
    fn on_text_block_complete(&mut self, full_text: &str) {
        self.inner.on_text_block_complete(full_text);
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
    pub thinking_max_tokens: u32,
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
    let api_messages: Vec<serde_json::Value> = messages
        .iter()
        .filter_map(|m| serde_json::to_value(m).ok())
        .collect();

    let request = CreateMessageRequest::new(
        params.model,
        params.max_tokens,
        params.thinking_max_tokens,
        system.to_vec(),
        api_messages,
        tool_schemas.to_vec(),
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
                        handler.on_text_block_complete(text);
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
                        .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));
                    let idx = content_blocks.len();
                    handler.on_tool_use_start(&name, idx);
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
    };

    let stop_reason_str = body
        .get("stop_reason")
        .and_then(|v| v.as_str())
        .unwrap_or("end_turn");

    Ok(StreamResponse {
        assistant_message: Message {
            role: Role::Assistant,
            content: content_blocks,
        },
        usage,
        stop_reason: StopReason::from_str(stop_reason_str),
    })
}
