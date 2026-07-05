//! Stream parsing utilities for Anthropic API format

use crate::business::types::*;
use futures_util::StreamExt;
use reqwest::Response;
use share::message::{ContentBlock, Message, Role};
use std::io;
use tokio::io::AsyncBufReadExt;
use tokio_util::io::StreamReader;
use tokio_util::sync::CancellationToken;

// Re-export StreamHandler from provider module
pub use crate::core::provider::StreamHandler;

/// Parse Anthropic-style SSE stream
pub async fn parse_stream(
    response: Response,
    handler: &mut dyn StreamHandler,
    cancel: &CancellationToken,
) -> Result<StreamResponse, crate::LlmError> {
    let mut content_blocks: Vec<ContentBlock> = Vec::new();
    let mut current_text = String::new();
    let mut current_thinking = String::new();
    let mut current_tool_id = String::new();
    let mut current_tool_name = String::new();
    let mut current_tool_json = String::new();
    let mut usage = Usage {
        input_tokens: 0,
        output_tokens: 0,
        cached_tokens: None,
        cache_creation_tokens: None,
        reasoning_tokens: None,
        total_tokens: None,
    };
    let mut stop_reason = StopReason::EndTurn;

    const STREAM_IDLE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(90);
    const STALL_THRESHOLD: std::time::Duration = std::time::Duration::from_secs(30);
    let mut last_event_time: Option<std::time::Instant> = None;
    let mut tool_index: usize = 0;

    let byte_stream = response.bytes_stream().map(|r| r.map_err(io::Error::other));
    let reader = StreamReader::new(byte_stream);
    let mut lines = reader.lines();

    loop {
        // Calculate remaining idle timeout based on time since last event
        let idle_deadline = match last_event_time {
            Some(last) => last + STREAM_IDLE_TIMEOUT,
            None => std::time::Instant::now() + STREAM_IDLE_TIMEOUT,
        };
        let remaining = idle_deadline.saturating_duration_since(std::time::Instant::now());

        let line = tokio::select! {
            biased;
            _ = cancel.cancelled() => {
                return Err(crate::LlmError::Cancelled);
            }
            _ = tokio::time::sleep(remaining) => {
                handler.on_error(&format!("Stream idle timeout: no data for {}s", STREAM_IDLE_TIMEOUT.as_secs()));
                return Err(crate::LlmError::Stream(format!(
                    "Stream idle timeout: no data received for {}s", STREAM_IDLE_TIMEOUT.as_secs()
                )));
            }
            result = lines.next_line() => {
                match result.map_err(|e| crate::LlmError::Stream(e.to_string()))? {
                    Some(line) => line,
                    None => break,
                }
            }
        };

        // Stall detection
        let now = std::time::Instant::now();
        if let Some(last) = last_event_time {
            let gap = now.duration_since(last);
            if gap > STALL_THRESHOLD {
                // Stream stall detected — silently ignored
            }
        }
        last_event_time = Some(now);

        handler.on_raw_line(&line);

        // 兼容 "data: {...}" (Anthropic) 和 "data:{...}" (DashScope)
        let data = if let Some(stripped) = line.strip_prefix("data: ") {
            stripped
        } else if let Some(stripped) = line.strip_prefix("data:") {
            stripped
        } else {
            continue;
        };
        if data == "[DONE]" {
            break;
        }

        let event: StreamEvent = match serde_json::from_str(data) {
            Ok(e) => e,
            Err(_) => continue,
        };

        match event {
            StreamEvent::MessageStart { message: msg } => {
                usage = msg.usage;
            }
            StreamEvent::ContentBlockStart { content_block, .. } => {
                match content_block {
                    ContentBlockPayload::Text { text } => {
                        current_text = text;
                    }
                    ContentBlockPayload::ToolUse { id, name } => {
                        current_tool_id = id;
                        current_tool_name = name.clone();
                        current_tool_json.clear();
                        handler.on_tool_use_start(&name, Some(&current_tool_id), tool_index);
                        tool_index += 1;
                    }
                    ContentBlockPayload::Thinking { thinking } => {
                        current_thinking = thinking;
                    }
                    ContentBlockPayload::Unknown => {
                        // ignore unknown block types
                    }
                }
            }
            StreamEvent::ContentBlockDelta { delta, .. } => {
                match delta {
                    DeltaPayload::TextDelta { text } => {
                        handler.on_text(&text);
                        current_text.push_str(&text);
                    }
                    DeltaPayload::InputJsonDelta { partial_json } => {
                        current_tool_json.push_str(&partial_json);
                        if !current_tool_name.is_empty() {
                            handler.on_tool_arguments_delta(
                                tool_index.saturating_sub(1),
                                &current_tool_name,
                                Some(&current_tool_id),
                                &current_tool_json,
                            );
                        }
                    }
                    DeltaPayload::ThinkingDelta { thinking } => {
                        current_thinking.push_str(&thinking);
                    }
                    DeltaPayload::SignatureDelta { .. } | DeltaPayload::Unknown => {
                        // ignored
                    }
                }
            }
            StreamEvent::ContentBlockStop { .. } => {
                if !current_tool_id.is_empty() {
                    let input: serde_json::Value = match serde_json::from_str(&current_tool_json) {
                        Ok(v) => v,
                        Err(_) => {
                            // 截断恢复：尝试补全被切在字符串字面量中间的 arguments JSON。
                            if let Some(recovered) =
                                crate::business::json_recovery::try_complete_truncated_json(
                                    &current_tool_json,
                                )
                            {
                                log::warn!(
                                    target: "aemeath:agent:provider",
                                    "Anthropic 流式 tool_call JSON 解析失败但启发式恢复成功（{} bytes → {} bytes）",
                                    current_tool_json.len(),
                                    serde_json::to_string(&recovered).map(|s| s.len()).unwrap_or(0),
                                );
                                recovered
                            } else {
                                let head_preview: String =
                                    current_tool_json.chars().take(200).collect();
                                let tail_preview: String = current_tool_json
                                    .chars()
                                    .rev()
                                    .take(200)
                                    .collect::<String>()
                                    .chars()
                                    .rev()
                                    .collect();
                                return Err(crate::LlmError::StreamTruncated {
                                    tool_call_id: current_tool_id.clone(),
                                    tool_call_name: current_tool_name.clone(),
                                    accumulated_bytes: current_tool_json.len(),
                                    delta_count: 0,
                                    head_preview,
                                    tail_preview,
                                });
                            }
                        }
                    };
                    content_blocks.push(ContentBlock::ToolUse {
                        id: std::mem::take(&mut current_tool_id),
                        name: std::mem::take(&mut current_tool_name),
                        input,
                    });
                    current_tool_json.clear();
                } else if !current_thinking.is_empty() {
                    content_blocks.push(ContentBlock::Thinking {
                        thinking: std::mem::take(&mut current_thinking),
                    });
                } else if !current_text.is_empty() {
                    handler.on_block_complete(&current_text);
                    content_blocks.push(ContentBlock::Text {
                        text: std::mem::take(&mut current_text),
                    });
                }
            }
            StreamEvent::MessageDelta {
                delta,
                usage: delta_usage,
            } => {
                if let Some(reason) = delta.stop_reason {
                    stop_reason = StopReason::parse(&reason);
                }
                if let Some(du) = delta_usage {
                    usage.output_tokens = du.output_tokens;
                }
            }
            StreamEvent::Error { error } => {
                handler.on_error(&error.message);
                return Err(crate::LlmError::Api {
                    error_type: error.error_type,
                    message: error.message,
                });
            }
            StreamEvent::MessageStop | StreamEvent::Ping => {}
        }
    }

    Ok(StreamResponse {
        assistant_message: Message {
            role: Role::Assistant,
            content: content_blocks,
            metadata: None,
        },
        usage,
        stop_reason,
    })
}
