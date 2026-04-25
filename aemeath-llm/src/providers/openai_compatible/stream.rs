//! 流式解析：解析 OpenAI 风格的 SSE 流

use aemeath_core::message::{ContentBlock, Message, Role};
use futures_util::StreamExt;
use std::io;
use tokio::io::AsyncBufReadExt;
use tokio_util::io::StreamReader;
use tokio_util::sync::CancellationToken;
use crate::provider::StreamHandler;
use crate::types::StreamResponse;

/// 流空闲超时：90 秒无数据则中止
pub(crate) const STREAM_IDLE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(90);
/// 停滞检测阈值：超过 30 秒无数据则记录警告
pub(crate) const STALL_THRESHOLD: std::time::Duration = std::time::Duration::from_secs(30);

/// 解析 OpenAI 风格的 SSE 流
pub(crate) async fn parse_openai_stream(
    response: reqwest::Response,
    handler: &mut dyn StreamHandler,
    cancel: &CancellationToken,
) -> Result<StreamResponse, crate::LlmError> {
    let mut content_blocks: Vec<ContentBlock> = Vec::new();
    let mut current_text = String::new();
    let mut current_reasoning = String::new();
    let mut current_tool_calls: std::collections::HashMap<usize, (String, String, String)> = std::collections::HashMap::new();
    let mut usage = crate::types::Usage { input_tokens: 0, output_tokens: 0 };
    let mut stop_reason = crate::types::StopReason::EndTurn;
    let mut last_event_time: Option<std::time::Instant> = None;

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

        // 停滞检测
        let now = std::time::Instant::now();
        if let Some(last) = last_event_time {
            let gap = now.duration_since(last);
            if gap > STALL_THRESHOLD {
                // 检测到流停滞 — 静默忽略
            }
        }
        last_event_time = Some(now);
        handler.on_raw_line(&line);

        // 解析 SSE 格式
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

        let chunk: serde_json::Value = match serde_json::from_str(data) {
            Ok(v) => v,
            Err(_) => continue,
        };

        // 检查错误
        if let Some(error) = chunk.get("error") {
            let error_msg = error.get("message").and_then(|m| m.as_str()).unwrap_or("Unknown error");
            handler.on_error(error_msg);
            return Err(crate::LlmError::Api {
                error_type: "api_error".to_string(),
                message: error_msg.to_string(),
            });
        }

        // 提取 usage（某些 provider 在最后一个 chunk 中包含）
        if let Some(usage_obj) = chunk.get("usage") {
            if !usage_obj.is_null() {
                let in_tok = usage_obj.get("prompt_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                let out_tok = usage_obj.get("completion_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                if in_tok > 0 || out_tok > 0 {
                    usage.input_tokens = in_tok;
                    usage.output_tokens = out_tok;
                }
            }
        }

        // 处理 choices
        if let Some(choices) = chunk.get("choices").and_then(|c| c.as_array()) {
            for choice in choices {
                // 检查 finish_reason
                if let Some(finish) = choice.get("finish_reason").and_then(|f| f.as_str()) {
                    stop_reason = match finish {
                        "stop" => crate::types::StopReason::EndTurn,
                        "tool_calls" => crate::types::StopReason::ToolUse,
                        "length" => crate::types::StopReason::MaxTokens,
                        _ => crate::types::StopReason::EndTurn,
                    };
                }

                // 处理 delta
                if let Some(delta) = choice.get("delta") {
                    // Reasoning 内容（例如 glm-5.1, DeepSeek-R1）
                    if let Some(reasoning) = delta.get("reasoning_content").and_then(|c| c.as_str()) {
                        if !reasoning.is_empty() {
                            handler.on_thinking(reasoning);
                            current_reasoning.push_str(reasoning);
                        }
                    }

                    // 文本内容
                    if let Some(content) = delta.get("content").and_then(|c| c.as_str()) {
                        handler.on_text(content);
                        current_text.push_str(content);
                    }

                    // Tool calls
                    if let Some(tool_calls) = delta.get("tool_calls").and_then(|t| t.as_array()) {
                        for tc in tool_calls {
                            let index = tc.get("index").and_then(|i| i.as_u64()).unwrap_or(0) as usize;

                            // 获取或创建 tool call 条目
                            let entry = current_tool_calls.entry(index).or_insert_with(|| {
                                (String::new(), String::new(), String::new())
                            });

                            // 如果存在则更新 ID
                            if let Some(id) = tc.get("id").and_then(|i| i.as_str()) {
                                entry.0 = id.to_string();
                            }

                            // 更新 function 信息
                            if let Some(function) = tc.get("function") {
                                if let Some(name) = function.get("name").and_then(|n| n.as_str()) {
                                    entry.1 = name.to_string();
                                    if entry.0.is_empty() {
                                        // 某些 provider 不发送 tool call ID
                                        entry.0 = format!("call_{}", index);
                                    }
                                }
                                if let Some(args) = function.get("arguments").and_then(|a| a.as_str()) {
                                    entry.2.push_str(args);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // 构建最终的 content blocks。
    // Thinking 块必须在 Text 之前，以便 convert_messages 在重发 assistant 历史
    // 时能正确地将 reasoning_content 附加到拥有内容的消息上（对 DeepSeek thinking 模式很重要）。
    if !current_reasoning.is_empty() {
        content_blocks.push(ContentBlock::Thinking {
            thinking: current_reasoning,
        });
    }
    if !current_text.is_empty() {
        handler.on_text_block_complete(&current_text);
        content_blocks.push(ContentBlock::Text {
            text: current_text,
        });
    }

    // 按 index 排序 tool calls 并添加到 content
    let mut sorted_tool_calls: Vec<_> = current_tool_calls.into_iter().collect();
    sorted_tool_calls.sort_by_key(|(i, _)| *i);

    for (_, (id, name, arguments)) in sorted_tool_calls {
        if !name.is_empty() {
            handler.on_tool_use_start(&name);
            let input: serde_json::Value = serde_json::from_str(&arguments)
                .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));
            content_blocks.push(ContentBlock::ToolUse { id, name, input });
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
