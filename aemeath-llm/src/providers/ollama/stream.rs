//! 流式响应解析：解析 Ollama 原生 `/api/chat` NDJSON 流。

use aemeath_core::message::{ContentBlock, Message, Role};
use futures_util::StreamExt;
use std::io;
use tokio::io::AsyncBufReadExt;
use tokio_util::io::StreamReader;
use tokio_util::sync::CancellationToken;

use crate::provider::StreamHandler;
use crate::types::StreamResponse;
use super::STREAM_IDLE_TIMEOUT;

/// Parse ollama's native `/api/chat` NDJSON stream.
///
/// Stream format: one JSON object per line, no `data:` prefix, no `[DONE]`.
/// Each chunk: `{message:{role,content,thinking?,tool_calls?}, done, done_reason?, prompt_eval_count?, eval_count?}`.
/// Tool calls typically arrive in the final `done:true` chunk for qwen3-style models.
pub(crate) async fn parse_ollama_stream(
    response: reqwest::Response,
    handler: &mut dyn StreamHandler,
    cancel: &CancellationToken,
) -> Result<StreamResponse, crate::LlmError> {
    let mut content_blocks: Vec<ContentBlock> = Vec::new();
    let mut current_text = String::new();
    let mut final_tool_calls: Vec<(String, String, serde_json::Value)> = Vec::new();
    let mut usage = crate::types::Usage {
        input_tokens: 0,
        output_tokens: 0,
    };
    let mut stop_reason = crate::types::StopReason::EndTurn;

    let byte_stream = response
        .bytes_stream()
        .map(|r| r.map_err(|e| io::Error::new(io::ErrorKind::Other, e)));
    let reader = StreamReader::new(byte_stream);
    let mut lines = reader.lines();

    loop {
        let line = tokio::select! {
            biased;
            _ = cancel.cancelled() => {
                return Err(crate::LlmError::Stream("interrupted by user".to_string()));
            }
            _ = tokio::time::sleep(STREAM_IDLE_TIMEOUT) => {
                handler.on_error(&format!("Ollama stream idle timeout: no data for {}s", STREAM_IDLE_TIMEOUT.as_secs()));
                return Err(crate::LlmError::Stream(format!(
                    "Ollama stream idle timeout: no data for {}s — model may have stalled", STREAM_IDLE_TIMEOUT.as_secs()
                )));
            }
            result = lines.next_line() => {
                match result.map_err(|e| crate::LlmError::Stream(e.to_string()))? {
                    Some(line) => line,
                    None => break,
                }
            }
        };

        if line.trim().is_empty() {
            continue;
        }
        log::trace!("[ollama stream] <- {}", line);
        handler.on_raw_line(&line);

        // Native NDJSON: parse each non-empty line as a JSON object
        let chunk: serde_json::Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                log::debug!("[ollama stream] unparseable line ({}): {}", e, line);
                continue;
            }
        };

        // Stream-level error (ollama surfaces errors with an "error" key)
        if let Some(error) = chunk.get("error").and_then(|e| e.as_str()) {
            handler.on_error(error);
            return Err(crate::LlmError::Api {
                error_type: "ollama_error".to_string(),
                message: error.to_string(),
            });
        }

        if let Some(message) = chunk.get("message") {
            // Thinking delta
            if let Some(thinking) = message.get("thinking").and_then(|v| v.as_str()) {
                if !thinking.is_empty() {
                    handler.on_thinking(thinking);
                }
            }

            // Content delta
            if let Some(content) = message.get("content").and_then(|v| v.as_str()) {
                if !content.is_empty() {
                    handler.on_text(content);
                    current_text.push_str(content);
                }
            }

            // Tool calls (typically in the final done:true chunk)
            if let Some(tool_calls) = message.get("tool_calls").and_then(|t| t.as_array()) {
                for (idx, tc) in tool_calls.iter().enumerate() {
                    if let Some(function) = tc.get("function") {
                        let id = tc
                            .get("id")
                            .and_then(|i| i.as_str())
                            .map(|s| s.to_string())
                            .unwrap_or_else(|| format!("call_{}", idx));
                        let name = function
                            .get("name")
                            .and_then(|n| n.as_str())
                            .unwrap_or("")
                            .to_string();
                        let input = function
                            .get("arguments")
                            .cloned()
                            .unwrap_or_else(|| {
                                serde_json::Value::Object(serde_json::Map::new())
                            });
                        if !name.is_empty() {
                            final_tool_calls.push((id, name, input));
                        }
                    }
                }
            }
        }

        // Final chunk: done=true carries usage + done_reason
        if chunk.get("done").and_then(|v| v.as_bool()).unwrap_or(false) {
            if let Some(reason) = chunk.get("done_reason").and_then(|v| v.as_str()) {
                stop_reason = match reason {
                    "stop" => crate::types::StopReason::EndTurn,
                    "length" => crate::types::StopReason::MaxTokens,
                    _ => crate::types::StopReason::EndTurn,
                };
            }
            if let Some(n) = chunk.get("prompt_eval_count").and_then(|v| v.as_u64()) {
                usage.input_tokens = n as u32;
            }
            if let Some(n) = chunk.get("eval_count").and_then(|v| v.as_u64()) {
                usage.output_tokens = n as u32;
            }
            // Tool calls override the stop reason
            if !final_tool_calls.is_empty() {
                stop_reason = crate::types::StopReason::ToolUse;
            }
            break;
        }
    }

    let text_len = current_text.len();
    let tool_count = final_tool_calls.len();

    // Build final content blocks
    if !current_text.is_empty() {
        handler.on_text_block_complete(&current_text);
        content_blocks.push(ContentBlock::Text {
            text: current_text,
        });
    }

    for (id, name, input) in final_tool_calls {
        handler.on_tool_use_start(&name);
        content_blocks.push(ContentBlock::ToolUse { id, name, input });
    }

    log::debug!(
        "[ollama stream] done text_bytes={} tool_calls={} stop={:?} in_tok={} out_tok={}",
        text_len, tool_count, stop_reason, usage.input_tokens, usage.output_tokens
    );

    Ok(StreamResponse {
        assistant_message: Message {
            role: Role::Assistant,
            content: content_blocks,
        },
        usage,
        stop_reason,
    })
}
