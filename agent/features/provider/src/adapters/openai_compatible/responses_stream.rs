//! Responses API streaming 解析（/v1/responses SSE）
//!
//! 关键事件类型：
//! - `response.output_text.delta` — 文本增量
//! - `response.function_call_arguments.delta` — tool call 参数增量
//! - `response.output_item.added` — 新 item（function_call 或 message）
//! - `response.completed` — 含 usage

use super::usage::parse_responses_usage;
use crate::domain::invoke::{StopReason, StreamResponse, Usage};
use crate::ports::LegacyStreamSink;
use futures_util::StreamExt;
use share::message::{ContentBlock, Message, Role};
use std::io;
use tokio::io::AsyncBufReadExt;
use tokio_util::io::StreamReader;
use tokio_util::sync::CancellationToken;

/// 解析 Responses API SSE 流
pub(crate) async fn parse_responses_stream(
    response: reqwest::Response,
    handler: &mut dyn LegacyStreamSink,
    cancel: &CancellationToken,
) -> Result<StreamResponse, crate::LlmError> {
    let mut content_blocks: Vec<ContentBlock> = Vec::new();
    let mut current_text = String::new();
    // (output_index → (call_id, name, arguments))
    let mut function_calls: std::collections::HashMap<usize, (String, String, String)> =
        std::collections::HashMap::new();
    let mut usage = Usage {
        input_tokens: 0,
        output_tokens: 0,
        cached_tokens: None,
        cache_creation_tokens: None,
        reasoning_tokens: None,
        total_tokens: None,
    };
    let mut stop_reason = StopReason::EndTurn;

    let byte_stream = response.bytes_stream().map(|r| {
        r.map_err(|e| {
            let mut msg = format!("{}", e);
            let mut source = std::error::Error::source(&e);
            let mut depth = 1;
            while let Some(cause) = source {
                msg.push_str(&format!("\n  Cause #{}: {}", depth, cause));
                source = cause.source();
                depth += 1;
            }
            io::Error::other(msg)
        })
    });
    let mut reader = tokio::io::BufReader::new(StreamReader::new(byte_stream));
    let mut buf = String::new();

    loop {
        tokio::select! {
            biased;
            _ = cancel.cancelled() => {
                return Err(crate::LlmError::Cancelled);
            }
            result = reader.read_line(&mut buf) => {
                match result {
                    Ok(0) => break,
                    Ok(_) => {}
                    Err(e) => {
                        return Err(crate::LlmError::Stream(e.to_string()));
                    }
                }
            }
        }

        let line = buf.trim().to_string();
        buf.clear();
        handler.on_raw_line(&line);

        if line.is_empty() {
            continue;
        }
        if !line.starts_with("data: ") {
            continue;
        }

        let data = &line[6..];
        if data == "[DONE]" {
            break;
        }

        let event: serde_json::Value = match serde_json::from_str(data) {
            Ok(v) => v,
            Err(e) => {
                log::debug!(target: crate::LOG_TARGET,
                    "[responses-stream] JSON parse error: {} | line: {}",
                    e, &data[..data.len().min(200)]
                );
                continue;
            }
        };

        let event_type = event.get("type").and_then(|t| t.as_str()).unwrap_or("");

        match event_type {
            "response.output_text.delta" => {
                if let Some(delta) = event.get("delta").and_then(|d| d.as_str()) {
                    handler.on_text(delta);
                    current_text.push_str(delta);
                }
            }

            "response.output_item.added" => {
                let output_index = event
                    .get("output_index")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as usize;
                if let Some(item) = event.get("item") {
                    let item_type = item.get("type").and_then(|t| t.as_str()).unwrap_or("");
                    if item_type == "function_call" {
                        let call_id = item
                            .get("call_id")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let name = item
                            .get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        function_calls
                            .insert(output_index, (call_id.clone(), name.clone(), String::new()));
                        handler.on_tool_use_start(&name, Some(&call_id), output_index);
                    }
                }
            }

            "response.function_call_arguments.delta" => {
                let output_index = event
                    .get("output_index")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as usize;
                if let Some(delta) = event.get("delta").and_then(|d| d.as_str()) {
                    if let Some(entry) = function_calls.get_mut(&output_index) {
                        entry.2.push_str(delta);
                        let (call_id, name, args) = entry.clone();
                        handler.on_tool_arguments_delta(output_index, &name, Some(&call_id), &args);
                    }
                }
            }

            "response.function_call_arguments.done" => {
                let output_index = event
                    .get("output_index")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as usize;
                if let Some(args) = event.get("arguments").and_then(|d| d.as_str()) {
                    if let Some(entry) = function_calls.get_mut(&output_index) {
                        entry.2 = args.to_string();
                    }
                }
            }

            "response.completed" => {
                // 提取 usage
                if let Some(resp_obj) = event.get("response") {
                    if let Some(u) = resp_obj.get("usage") {
                        usage = parse_responses_usage(u);
                    }

                    // 检查 status 判断 stop_reason
                    let status = resp_obj
                        .get("status")
                        .and_then(|s| s.as_str())
                        .unwrap_or("completed");
                    if status == "incomplete" {
                        stop_reason = StopReason::MaxTokens;
                    }

                    // 检查 output items 判断是否有 tool calls
                    if let Some(output) = resp_obj.get("output").and_then(|o| o.as_array()) {
                        let has_tool_calls = output.iter().any(|item| {
                            item.get("type").and_then(|t| t.as_str()) == Some("function_call")
                        });
                        if has_tool_calls {
                            stop_reason = StopReason::ToolUse;
                        }
                    }
                }
            }

            _ => {
                // 忽略其他事件类型（response.created, response.in_progress, 等）
            }
        }
    }

    // 组装 content blocks
    if !current_text.is_empty() {
        content_blocks.push(ContentBlock::Text { text: current_text });
    }

    // 按 output_index 排序 function calls
    let mut sorted_indices: Vec<usize> = function_calls.keys().copied().collect();
    sorted_indices.sort();
    for idx in sorted_indices {
        if let Some((id, name, args_str)) = function_calls.remove(&idx) {
            let input: serde_json::Value =
                serde_json::from_str(&args_str).unwrap_or(serde_json::json!({}));
            content_blocks.push(ContentBlock::ToolUse { id, name, input });
        }
    }

    let assistant_message = Message {
        role: Role::Assistant,
        content: content_blocks,
        metadata: None,
    };

    usage.finalize_total_tokens(0);

    Ok(StreamResponse {
        assistant_message,
        usage,
        stop_reason,
    })
}
