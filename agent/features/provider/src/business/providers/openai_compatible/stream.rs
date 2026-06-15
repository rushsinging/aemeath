//! 流式解析：解析 OpenAI 风格的 SSE 流

use crate::business::types::StreamResponse;
use crate::core::provider::StreamHandler;
use futures_util::StreamExt;
use share::message::{ContentBlock, Message, Role};
use std::io;
use tokio::io::AsyncBufReadExt;
use tokio_util::io::StreamReader;
use tokio_util::sync::CancellationToken;

/// 流空闲超时：90 秒无数据则中止
pub(crate) const STREAM_IDLE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(90);
/// 停滞检测阈值：超过 30 秒无数据则记录警告
pub(crate) const STALL_THRESHOLD: std::time::Duration = std::time::Duration::from_secs(30);

/// 尝试通过补全 closing quote 和必要的右括号，从上游截断的 JSON 字符串中恢复出可解析的 JSON。
///
/// 适用场景：上游 SSE 流在某个 tool_call `arguments` 字符串字面量中间被截断（典型 EOF 错误）。
/// 该情况是 OpenAI 兼容 provider 最常见的流式截断形态，因为模型经常在 string 边界被切。
///
/// 启发式策略：
/// 1. 用状态机扫描原始字符串，跟踪"是否在 string 内"（正确处理 `\\` 和 `\"` 转义）。
/// 2. 仅当流结束**且**仍处于 string 中时尝试补全；其他截断形态（如缺逗号/冒号）不做猜测。
/// 3. 补 `"` 关闭 string，然后按未闭合的结构符顺序补 `}` / `]`。
/// 4. 重新调用 `serde_json::from_str`；成功则返回 `Some(value)`，失败返回 `None`（让 caller 走原错误路径）。
///
/// 注意：**绝不**对截断在结构边界（`,` `:` `{` `[` 之后）的情况做"猜测式补全"，
/// 因为那会引入 silent corruption（例如把 `{"a":1` 补成 `{"a":1}`，模型侧的语义可能完全不同）。
pub(crate) fn try_complete_truncated_json(raw: &str) -> Option<serde_json::Value> {
    let mut in_string = false;
    let mut escape = false;
    for &b in raw.as_bytes() {
        if escape {
            escape = false;
            continue;
        }
        match b {
            b'\\' if in_string => escape = true,
            b'"' => in_string = !in_string,
            _ => {}
        }
    }

    // 只处理"在 string 内被截断"这一种形态。其他形态让 caller 抛错。
    if !in_string {
        return None;
    }

    // 补一个 closing quote，然后遍历整段字符串统计未闭合的结构符。
    let mut candidate = String::with_capacity(raw.len() + 16);
    candidate.push_str(raw);
    candidate.push('"');

    let mut stack: Vec<u8> = Vec::new();
    let mut in_str2 = false;
    let mut esc2 = false;
    for &b in candidate.as_bytes() {
        if esc2 {
            esc2 = false;
            continue;
        }
        match b {
            b'\\' if in_str2 => esc2 = true,
            b'"' => in_str2 = !in_str2,
            b'{' if !in_str2 => stack.push(b'}'),
            b'[' if !in_str2 => stack.push(b']'),
            _ => {}
        }
    }

    // 按栈的逆序补右括号（即最深层的先闭合）。
    while let Some(c) = stack.pop() {
        candidate.push(c as char);
    }

    serde_json::from_str(&candidate).ok()
}

/// 解析 OpenAI 风格的 SSE 流
pub(crate) async fn parse_openai_stream(
    response: reqwest::Response,
    handler: &mut dyn StreamHandler,
    cancel: &CancellationToken,
) -> Result<StreamResponse, crate::LlmError> {
    let mut content_blocks: Vec<ContentBlock> = Vec::new();
    let mut current_text = String::new();
    let mut current_reasoning = String::new();
    // (id, name, arguments_str, delta_count) per index — delta_count is for diagnostics
    let mut current_tool_calls: std::collections::HashMap<usize, (String, String, String, u32)> =
        std::collections::HashMap::new();
    let mut usage = crate::business::types::Usage {
        input_tokens: 0,
        output_tokens: 0,
    };
    let mut stop_reason = crate::business::types::StopReason::EndTurn;
    let mut last_event_time: Option<std::time::Instant> = None;

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
    let reader = StreamReader::new(byte_stream);
    let mut lines = reader.lines();

    loop {
        let line = tokio::select! {
            biased;
            _ = cancel.cancelled() => {
                return Err(crate::LlmError::Cancelled);
            }
            _ = tokio::time::sleep(STREAM_IDLE_TIMEOUT) => {
                handler.on_error(&format!("Stream idle timeout: no data for {}s", STREAM_IDLE_TIMEOUT.as_secs()));
                return Err(crate::LlmError::Stream(format!(
                    "Stream idle timeout: no data received for {}s", STREAM_IDLE_TIMEOUT.as_secs()
                )));
            }
            result = lines.next_line() => {
                match result {
                    Ok(Some(line)) => line,
                    Ok(None) => break,
                    Err(e) => {
                        log::warn!(target: "provider::openai_stream", "[openai-compat stream] failed to read SSE line: {}", e);
                        return Err(crate::LlmError::Stream(e.to_string()));
                    }
                }            }
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

        let chunk: serde_json::Value = match serde_json::from_str(data) {
            Ok(v) => v,
            Err(_) => continue,
        };

        // 检查错误
        if let Some(error) = chunk.get("error") {
            let error_msg = error
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("Unknown error");
            handler.on_error(error_msg);
            return Err(crate::LlmError::Api {
                error_type: "api_error".to_string(),
                message: error_msg.to_string(),
            });
        }

        // 提取 usage（某些 provider 在最后一个 chunk 中包含）
        if let Some(usage_obj) = chunk.get("usage") {
            if !usage_obj.is_null() {
                let in_tok = usage_obj
                    .get("prompt_tokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as u32;
                let out_tok = usage_obj
                    .get("completion_tokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as u32;
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
                        "stop" => crate::business::types::StopReason::EndTurn,
                        "tool_calls" => crate::business::types::StopReason::ToolUse,
                        "length" => crate::business::types::StopReason::MaxTokens,
                        _ => crate::business::types::StopReason::EndTurn,
                    };
                }

                // 处理 delta
                if let Some(delta) = choice.get("delta") {
                    // Reasoning 内容（例如 glm-5.1, DeepSeek-R1）
                    if let Some(reasoning) = delta.get("reasoning_content").and_then(|c| c.as_str())
                    {
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
                            let index =
                                tc.get("index").and_then(|i| i.as_u64()).unwrap_or(0) as usize;

                            // 获取或创建 tool call 条目
                            let entry = current_tool_calls.entry(index).or_insert_with(|| {
                                (String::new(), String::new(), String::new(), 0)
                            });

                            // 如果存在则更新 ID
                            if let Some(id) = tc.get("id").and_then(|i| i.as_str()) {
                                entry.0 = id.to_string();
                            }

                            // 更新 function 信息
                            if let Some(function) = tc.get("function") {
                                if let Some(name) = function.get("name").and_then(|n| n.as_str()) {
                                    // First time we see this tool call's name — notify the
                                    // handler immediately so the UI can show it in real time.
                                    // Skip empty names: some providers send name="" in early
                                    // deltas before the real name arrives.
                                    let is_new = entry.1.is_empty() && !name.is_empty();
                                    if !name.is_empty() {
                                        entry.1 = name.to_string();
                                    }
                                    if entry.0.is_empty() {
                                        // 某些 provider 不发送 tool call ID
                                        entry.0 = format!("call_{}", index);
                                    }
                                    if is_new {
                                        handler.on_tool_use_start(name, Some(&entry.0), index);
                                    }
                                }
                                if let Some(args) =
                                    function.get("arguments").and_then(|a| a.as_str())
                                {
                                    entry.2.push_str(args);
                                    entry.3 += 1;
                                    // Notify handler with accumulated arguments for
                                    // real-time UI updates (e.g. showing file path).
                                    if !entry.1.is_empty() {
                                        handler.on_tool_arguments_delta(
                                            index,
                                            &entry.1,
                                            Some(&entry.0),
                                            &entry.2,
                                        );
                                    }
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
        handler.on_block_complete(&current_text);
        content_blocks.push(ContentBlock::Text { text: current_text });
    }

    // 按 index 排序 tool calls 并添加到 content
    let mut sorted_tool_calls: Vec<_> = current_tool_calls.into_iter().collect();
    sorted_tool_calls.sort_by_key(|(i, _)| *i);

    let mut truncated_tool: Option<(String, String, String, String, usize, u32)> = None;
    for (_, (id, name, arguments, delta_count)) in sorted_tool_calls {
        if name.is_empty() {
            log::warn!(target: "provider::openai_stream",
                "[openai-compat stream] tool_call entry with empty name: id={}, args_bytes={}, delta_count={} — skipping",
                id, arguments.len(), delta_count
            );
            continue;
        }
        // Note: on_tool_use_start was already called during streaming
        // when the name first appeared in a delta chunk. No need to
        // call it again here.
        let input: serde_json::Value = if arguments.is_empty() {
            log::warn!(target: "provider::openai_stream",
                "[openai-compat stream] tool_call '{}' (id={}) had NO arguments delta after {} chunks — model emitted name only. Falling back to {{}}.",
                name, id, delta_count
            );
            serde_json::Value::Object(serde_json::Map::new())
        } else {
            match serde_json::from_str(&arguments) {
                Ok(v) => v,
                Err(e) => {
                    let is_eof = matches!(e.classify(), serde_json::error::Category::Eof);

                    // 先尝试启发式补全：仅当 JSON 在字符串字面量中间被截断时有效。
                    if let Some(recovered) = try_complete_truncated_json(&arguments) {
                        log::warn!(target: "provider::openai_stream",
                            "[openai-compat stream] tool_call '{}' (id={}) arguments truncated mid-string but heuristic recovery succeeded after {} delta chunks ({} bytes) — using recovered JSON. (Original error: {})",
                            name, id, delta_count, arguments.len(), e
                        );
                        recovered
                    } else {
                        let head: String = arguments.chars().take(300).collect();
                        let tail_rev: String =
                            arguments.chars().rev().take(200).collect::<String>();
                        let tail: String = tail_rev.chars().rev().collect();
                        log::warn!(target: "provider::openai_stream",
                            "[openai-compat stream] tool_call '{}' (id={}) arguments parse failed after {} delta chunks ({} bytes): {} — heuristic recovery also failed.",
                            name, id, delta_count, arguments.len(), e
                        );
                        log::warn!(target: "provider::openai_stream", "[openai-compat stream] truncated args head: {}", head);
                        log::warn!(target: "provider::openai_stream", "[openai-compat stream] truncated args tail: {}", tail);
                        if is_eof && truncated_tool.is_none() {
                            truncated_tool = Some((
                                id.clone(),
                                name.clone(),
                                head,
                                tail,
                                arguments.len(),
                                delta_count,
                            ));
                        }
                        serde_json::Value::Object(serde_json::Map::new())
                    }
                }
            }
        };
        content_blocks.push(ContentBlock::ToolUse { id, name, input });
    }

    // 如果 args 因 EOF 截断（典型上游断流症状）且启发式补全也失败，向上抛结构化错误
    // 让 caller 决定下一步（重试 stream / fallback non-streaming）。
    // **不再**给模型送 `{}`，因为那会陷入"missing required parameter"死循环。
    if let Some((tid, tname, head, tail, raw_len, delta_count)) = truncated_tool {
        return Err(crate::LlmError::StreamTruncated {
            tool_call_id: tid,
            tool_call_name: tname,
            accumulated_bytes: raw_len,
            delta_count,
            head_preview: head,
            tail_preview: tail,
        });
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

#[cfg(test)]
mod json_recovery_tests {
    use super::try_complete_truncated_json;
    use serde_json::json;

    #[test]
    fn recovers_when_string_value_is_truncated_mid_quote() {
        // 模型写到 `"file_path":"/Users/...` 后流被切断
        let raw = r#"{"file_path":"/Users/x"#;
        let recovered = try_complete_truncated_json(raw).expect("应该能补全");
        assert_eq!(recovered, json!({"file_path": "/Users/x"}));
    }

    #[test]
    fn recovers_when_string_value_contains_escape_sequences() {
        // 字符串中含有 `\"` 和 `\\` 转义，不应让状态机误判
        let raw = r#"{"content":"line1\nline2 \"with quote\""#;
        let recovered = try_complete_truncated_json(raw).expect("应该能补全");
        assert_eq!(recovered["content"], "line1\nline2 \"with quote\"");
    }

    #[test]
    fn recovers_nested_objects() {
        // 嵌套对象，截断在最里层 string
        let raw = r#"{"outer":{"inner":{"key":"val"#;
        let recovered = try_complete_truncated_json(raw).expect("应该能补全");
        assert_eq!(recovered, json!({"outer": {"inner": {"key": "val"}}}));
    }

    #[test]
    fn recovers_arrays_inside_object() {
        // 数组作为 value，且 array 内的 string 也被截断
        let raw = r#"{"items":["a","b","c"#;
        let recovered = try_complete_truncated_json(raw).expect("应该能补全");
        assert_eq!(recovered, json!({"items": ["a", "b", "c"]}));
    }

    #[test]
    fn does_not_recover_when_truncated_outside_a_string() {
        // 截断在结构符之后（缺逗号），不做猜测 — 避免 silent corruption
        let raw = r#"{"a":1"#;
        assert!(try_complete_truncated_json(raw).is_none());
    }

    #[test]
    fn does_not_recover_well_formed_json() {
        // 正常 JSON：状态机结束在 string 之外（in_string=false），不触发补全
        let raw = r#"{"a":1,"b":"ok"}"#;
        assert!(try_complete_truncated_json(raw).is_none());
    }

    #[test]
    fn does_not_recover_when_closing_quote_would_be_invalid() {
        // 流刚好在合法 string 末尾被切，再补一个 `"` 会破坏语法；
        // 我们的状态机此时 in_string=false（最后一个 `"` 已关），所以不补。
        // 这是 expected 行为 — 这种情况下 JSON 实际上是 well-formed 的（仅缺 closing brace）。
        let raw = r#"{"a":"b""#;
        assert!(try_complete_truncated_json(raw).is_none());
    }

    #[test]
    fn does_not_recover_completely_empty_input() {
        assert!(try_complete_truncated_json("").is_none());
    }
}
