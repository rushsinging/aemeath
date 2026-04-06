//! Stream parsing utilities for Anthropic API format

use crate::types::*;
use aemeath_core::message::{ContentBlock, Message, Role};
use futures_util::StreamExt;
use reqwest::Response;
use std::io;
use tokio::io::AsyncBufReadExt;
use tokio_util::io::StreamReader;
use tokio_util::sync::CancellationToken;

// Re-export StreamHandler from provider module
pub use crate::provider::StreamHandler;

/// Parse Anthropic-style SSE stream
pub async fn parse_stream(
    response: Response,
    handler: &mut dyn StreamHandler,
    cancel: &CancellationToken,
) -> Result<StreamResponse, crate::LlmError> {
    let mut content_blocks: Vec<ContentBlock> = Vec::new();
    let mut current_text = String::new();
    let mut current_tool_id = String::new();
    let mut current_tool_name = String::new();
    let mut current_tool_json = String::new();
    let mut usage = Usage { input_tokens: 0, output_tokens: 0 };
    let mut stop_reason = StopReason::EndTurn;

    const STREAM_IDLE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(90);
    const STALL_THRESHOLD: std::time::Duration = std::time::Duration::from_secs(30);
    let mut last_event_time: Option<std::time::Instant> = None;
    let mut stall_count: u32 = 0;

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
                eprintln!("[warn] Stream idle timeout: no data for {}s, aborting", STREAM_IDLE_TIMEOUT.as_secs());
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
                stall_count += 1;
                eprintln!("[warn] Stream stall #{}: {:.1}s gap between events", stall_count, gap.as_secs_f64());
            }
        }
        last_event_time = Some(now);

        handler.on_raw_line(&line);

        // 兼容 "data: {...}" (Anthropic) 和 "data:{...}" (DashScope)
        let data = if line.starts_with("data: ") {
            &line[6..]
        } else if line.starts_with("data:") {
            &line[5..]
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
                        handler.on_tool_use_start(&name);
                    }
                    ContentBlockPayload::Thinking { .. } | ContentBlockPayload::Unknown => {
                        // thinking blocks and unknown types are ignored
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
                    }
                    DeltaPayload::ThinkingDelta { .. }
                    | DeltaPayload::SignatureDelta { .. }
                    | DeltaPayload::Unknown => {
                        // ignored
                    }
                }
            }
            StreamEvent::ContentBlockStop { .. } => {
                if !current_tool_id.is_empty() {
                    let input: serde_json::Value = serde_json::from_str(&current_tool_json)
                        .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));
                    content_blocks.push(ContentBlock::ToolUse {
                        id: std::mem::take(&mut current_tool_id),
                        name: std::mem::take(&mut current_tool_name),
                        input,
                    });
                    current_tool_json.clear();
                } else if !current_text.is_empty() {
                    handler.on_text_block_complete(&current_text);
                    content_blocks.push(ContentBlock::Text {
                        text: std::mem::take(&mut current_text),
                    });
                }
            }
            StreamEvent::MessageDelta { delta, usage: delta_usage } => {
                if let Some(reason) = delta.stop_reason {
                    stop_reason = StopReason::from_str(&reason);
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
        },
        usage,
        stop_reason,
    })
}