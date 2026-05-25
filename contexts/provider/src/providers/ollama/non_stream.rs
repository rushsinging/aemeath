//! 非流式请求：通过 `/api/chat` 端点发送一次性请求。

use super::conversion::OllamaProviderConversion;
use super::OllamaProvider;
use crate::provider::StreamHandler;
use crate::types::{StreamResponse, SystemBlock};
use kernel::message::{ContentBlock, Message, Role};

pub(crate) trait OllamaProviderNonStream {
    async fn send_message_non_stream(
        &self,
        system: &[SystemBlock],
        messages: &[Message],
        tool_schemas: &[serde_json::Value],
        handler: &mut dyn StreamHandler,
    ) -> Result<StreamResponse, crate::LlmError>;
}

impl OllamaProviderNonStream for OllamaProvider {
    async fn send_message_non_stream(
        &self,
        system: &[SystemBlock],
        messages: &[Message],
        tool_schemas: &[serde_json::Value],
        handler: &mut dyn StreamHandler,
    ) -> Result<StreamResponse, crate::LlmError> {
        let request_body = self.build_request_body(system, messages, tool_schemas, false)?;
        let headers = self.build_headers()?;
        let url = format!("{}/api/chat", self.base_url);

        log::debug!(
            "[ollama non-stream] POST {} model={} think={} msgs={} tools={} body_bytes={}",
            url,
            self.model,
            self.reasoning.load(std::sync::atomic::Ordering::Relaxed),
            messages.len(),
            tool_schemas.len(),
            serde_json::to_string(&request_body)
                .map(|s| s.len())
                .unwrap_or(0),
        );

        let response = self
            .http
            .post(&url)
            .headers(headers)
            .json(&request_body)
            .send()
            .await
            .map_err(|e| {
                let detail = if e.is_connect() {
                    "connection failed"
                } else if e.is_timeout() {
                    "request timed out"
                } else if e.is_request() {
                    "request build error"
                } else {
                    "unknown"
                };
                let mut msg = format!("{} ({})\n  URL: {}", e, detail, url);
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

        let mut content_blocks = Vec::new();
        // ollama native usage: prompt_eval_count / eval_count at top level
        let input_tokens = body
            .get("prompt_eval_count")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32;
        let output_tokens = body.get("eval_count").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        let mut stop_reason = crate::types::StopReason::EndTurn;

        if let Some(done_reason) = body.get("done_reason").and_then(|v| v.as_str()) {
            stop_reason = match done_reason {
                "stop" => crate::types::StopReason::EndTurn,
                "length" => crate::types::StopReason::MaxTokens,
                _ => crate::types::StopReason::EndTurn,
            };
        }

        if let Some(message) = body.get("message") {
            // Thinking (reasoning) content — native field is `thinking`
            if let Some(thinking) = message.get("thinking").and_then(|v| v.as_str()) {
                if !thinking.is_empty() {
                    handler.on_thinking(thinking);
                }
            }

            if let Some(content) = message.get("content").and_then(|c| c.as_str()) {
                if !content.is_empty() {
                    handler.on_text(content);
                    handler.on_text_block_complete(content);
                    content_blocks.push(ContentBlock::Text {
                        text: content.to_string(),
                    });
                }
            }

            if let Some(tool_calls) = message.get("tool_calls").and_then(|t| t.as_array()) {
                if !tool_calls.is_empty() {
                    stop_reason = crate::types::StopReason::ToolUse;
                }
                for (idx, tool_call) in tool_calls.iter().enumerate() {
                    if let Some(function) = tool_call.get("function") {
                        let id = tool_call
                            .get("id")
                            .and_then(|i| i.as_str())
                            .map(|s| s.to_string())
                            .unwrap_or_else(|| format!("call_{}", idx));
                        let name = function
                            .get("name")
                            .and_then(|n| n.as_str())
                            .unwrap_or("")
                            .to_string();
                        // Native format: arguments is already a JSON object
                        let input = function
                            .get("arguments")
                            .cloned()
                            .unwrap_or_else(|| serde_json::Value::Object(serde_json::Map::new()));

                        handler.on_tool_use_start(&name, idx);
                        content_blocks.push(ContentBlock::ToolUse { id, name, input });
                    }
                }
            }
        }

        if content_blocks.is_empty() {
            return Err(crate::LlmError::Stream(
                "Ollama returned empty response (no text or tool calls)".to_string(),
            ));
        }

        Ok(StreamResponse {
            assistant_message: Message {
                role: Role::Assistant,
                content: content_blocks,
            },
            usage: crate::types::Usage {
                input_tokens,
                output_tokens,
            },
            stop_reason,
        })
    }
}
