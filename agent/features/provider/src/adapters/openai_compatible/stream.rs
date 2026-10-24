//! 流式解析：解析 OpenAI 风格的 SSE 流

use super::constants::{STALL_THRESHOLD, STREAM_IDLE_TIMEOUT};
use super::reasoning_normalizer::{self, ReasoningDeltaNormalizer};
use super::usage::parse_chat_usage;
use crate::adapters::stream::InvocationSink;
use crate::adapters::wire::StreamResponse;
use futures_util::StreamExt;
use share::message::{ContentBlock, Message, Role};
use std::io;
use tokio::io::AsyncBufReadExt;
use tokio_util::io::StreamReader;
use tokio_util::sync::CancellationToken;

// 截断 JSON 恢复函数已提取到 `provider::adapters::json_recovery`，
// 由 stream / non_stream / Anthropic 路径共享。

/// 单个 tool call 的流式累积状态。
///
/// 各 OpenAI 兼容网关对「本帧未提供该字段」有两种表达：**省略字段**（Aliyun）
/// 与**显式空串**（Qiniu）。两者语义等价，MUST 一并视为缺失，不得覆盖既有值——
/// 否则首帧给出的真实 `id` 会被后续空串冲掉，导致跨层关联断裂：TUI 收不到
/// 参数增量（goal 丢失、参数退化为原始 JSON）、`ToolCallCompleted` 无法收敛状态。
#[derive(Debug, Default, Clone)]
pub(crate) struct ToolCallAccum {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) arguments: String,
    pub(crate) delta_count: u32,
}

/// 合并一帧 `tool_calls` delta 后的判定结果。
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct ToolCallMergeOutcome {
    /// `name` 是否为本条目首次非空出现（用于实时建卡）。
    pub(crate) name_is_new: bool,
    /// 本帧是否携带 `arguments` 增量（决定是否推送参数预览）。
    pub(crate) has_arguments: bool,
}

impl ToolCallAccum {
    /// 合并一帧 `tool_calls` delta。
    ///
    /// - `id` / `name`：仅在非空时采纳（空串等同字段缺失）
    /// - `arguments`：只做追加
    pub(crate) fn merge(&mut self, _index: usize, tc: &serde_json::Value) -> ToolCallMergeOutcome {
        if let Some(id) = tc
            .get("id")
            .and_then(|value| value.as_str())
            .filter(|id| !id.is_empty())
        {
            self.id = id.to_string();
        }

        let mut outcome = ToolCallMergeOutcome::default();
        if let Some(function) = tc.get("function") {
            if let Some(name) = function
                .get("name")
                .and_then(|value| value.as_str())
                .filter(|name| !name.is_empty())
            {
                outcome.name_is_new = self.name.is_empty();
                self.name = name.to_string();
            }
            if let Some(arguments) = function.get("arguments").and_then(|value| value.as_str()) {
                self.arguments.push_str(arguments);
                self.delta_count += 1;
                outcome.has_arguments = true;
            }
        }
        outcome
    }

    /// 保证存在稳定 `id`：缺失时按 `index` 兜底。
    ///
    /// 兜底 MUST 在发出「跨层标识」前完成（`emit_tool_use_start` /
    /// `emit_tool_call_completed`），否则并行多路 tool call 会因 id 同为空而互相串错。
    pub(crate) fn ensure_id(&mut self, index: usize) {
        if self.id.is_empty() {
            self.id = format!("call_{}", index);
        }
    }
}

/// 格式化流状态快照，用于 idle timeout / error 诊断日志。
///
/// 输出形如：`text=42B reasoning=0B tools=[Write: 12KB/45d, Read: 0B/0d]`
fn format_stream_snapshot(
    text: &str,
    reasoning: &str,
    tool_calls: &std::collections::HashMap<usize, ToolCallAccum>,
) -> String {
    let mut parts: Vec<String> = Vec::new();
    parts.push(format!("text={}B", text.len()));
    parts.push(format!("reasoning={}B", reasoning.len()));
    if !tool_calls.is_empty() {
        let mut sorted: Vec<_> = tool_calls.iter().collect();
        sorted.sort_by_key(|(i, _)| **i);
        let tools_str: Vec<String> = sorted
            .iter()
            .map(|(_, accum)| {
                format!(
                    "{}: {}B/{}d",
                    accum.name,
                    accum.arguments.len(),
                    accum.delta_count
                )
            })
            .collect();
        parts.push(format!("tools=[{}]", tools_str.join(", ")));
    }
    parts.join(" ")
}

/// 解析 OpenAI 风格的 SSE 流
pub(crate) async fn parse_openai_stream(
    response: reqwest::Response,
    handler: &mut dyn InvocationSink,
    cancel: &CancellationToken,
) -> Result<StreamResponse, crate::LlmError> {
    let mut content_blocks: Vec<ContentBlock> = Vec::new();
    let mut current_text = String::new();
    let mut reasoning_normalizer = ReasoningDeltaNormalizer::new();
    // (id, name, arguments_str, delta_count) per index — delta_count is for diagnostics
    let mut current_tool_calls: std::collections::HashMap<usize, ToolCallAccum> =
        std::collections::HashMap::new();
    // #1494：已发出 ToolCallCompleted 的 index（index 切换先发，流结束兜底跳过）。
    let mut completed_tool_indices: std::collections::HashSet<usize> =
        std::collections::HashSet::new();
    let mut usage = crate::adapters::wire::Usage {
        input_tokens: 0,
        output_tokens: 0,
        cached_tokens: None,
        cache_creation_tokens: None,
        reasoning_tokens: None,
        total_tokens: None,
    };
    let mut stop_reason = crate::published_language::StopReason::EndTurn;
    let mut last_event_time: Option<std::time::Instant> = None;
    let mut chunk_index: u64 = 0;

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
                              let snapshot = format_stream_snapshot(
                                  &current_text, reasoning_normalizer.accumulated(), &current_tool_calls,
                              );
                              log::warn!(target: crate::LOG_TARGET,
                                  "[openai-compat stream] idle timeout: no data for {}s — {snapshot}",
                                  STREAM_IDLE_TIMEOUT.as_secs(),
                              );
                              handler.on_diagnostic(&format!(
                                  "Stream idle timeout: no data for {}s — {snapshot}",
                                  STREAM_IDLE_TIMEOUT.as_secs(),
                              ));
                              return Err(crate::LlmError::Stream(format!(
                                  "Stream idle timeout: no data received for {}s", STREAM_IDLE_TIMEOUT.as_secs()
                              )));
                          }
            result = lines.next_line() => {
                match result {
                    Ok(Some(line)) => line,
                    Ok(None) => break,
                    Err(e) => {
                        log::warn!(target: crate::LOG_TARGET, "[openai-compat stream] failed to read SSE line: {}", e);
                        return Err(crate::adapters::stream::stream_read_error(e));
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
        chunk_index += 1;

        // 检查错误
        if let Some(error) = chunk.get("error") {
            let error_msg = error
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("Unknown error");
            handler.on_diagnostic(error_msg);
            return Err(crate::LlmError::Api {
                error_type: "api_error".to_string(),
                message: error_msg.to_string(),
            });
        }

        // 提取 usage（某些 provider 在最后一个 chunk 中包含）
        if let Some(usage_obj) = chunk.get("usage") {
            if !usage_obj.is_null() {
                let parsed = parse_chat_usage(usage_obj);
                if parsed.input_tokens > 0 || parsed.output_tokens > 0 {
                    usage = parsed;
                }
            }
        }

        // 处理 choices
        if let Some(choices) = chunk.get("choices").and_then(|c| c.as_array()) {
            for choice in choices {
                // 检查 finish_reason
                if let Some(finish) = choice.get("finish_reason").and_then(|f| f.as_str()) {
                    stop_reason = match finish {
                        "stop" => crate::published_language::StopReason::EndTurn,
                        "tool_calls" => crate::published_language::StopReason::ToolUse,
                        "length" => crate::published_language::StopReason::MaxOutputTokens,
                        _ => crate::published_language::StopReason::EndTurn,
                    };
                }

                // 处理 delta
                if let Some(delta) = choice.get("delta") {
                    // Reasoning 内容（例如 glm-5.1, DeepSeek-R1, Mimo）
                    if let Some(reasoning) = delta.get("reasoning_content").and_then(|c| c.as_str())
                    {
                        if !reasoning.is_empty() {
                            // chunk 级诊断日志（debug/trace 级，安全 preview）
                            let raw_chars = reasoning.chars().count();
                            let raw_bytes = reasoning.len();
                            let acc_chars_before = reasoning_normalizer.accumulated_char_count();
                            log::trace!(target: crate::LOG_TARGET,
                                "[reasoning chunk] idx={} raw_chars={} raw_bytes={} \
                                 acc_chars_before={} acc_chars_after={}",
                                chunk_index, raw_chars, raw_bytes,
                                acc_chars_before, acc_chars_before + raw_chars,
                            );
                            if log::log_enabled!(log::Level::Trace) {
                                log::trace!(target: crate::LOG_TARGET,
                                    "[reasoning chunk] idx={} preview={}",
                                    chunk_index, reasoning_normalizer::safe_preview(reasoning),
                                );
                            }

                            // 归一化：处理 snapshot / 重复片段
                            let result = reasoning_normalizer.process(reasoning);
                            if !result.delta.is_empty() {
                                handler.emit_thinking(result.delta);
                            }
                            log::debug!(target: crate::LOG_TARGET,
                                "[reasoning chunk] idx={} dedup_action={:?} \
                                 raw_chars={} emitted_chars={} acc_chars={}",
                                chunk_index, result.action, raw_chars,
                                result.delta.chars().count(),
                                reasoning_normalizer.accumulated_char_count(),
                            );
                        }
                    }

                    // 文本内容
                    if let Some(content) = delta.get("content").and_then(|c| c.as_str()) {
                        if !content.is_empty() {
                            log::trace!(target: crate::LOG_TARGET,
                                "[content chunk] idx={} content_chars={} content_bytes={}",
                                chunk_index, content.chars().count(), content.len(),
                            );
                            handler.emit_text(content);
                            current_text.push_str(content);
                        }
                    }

                    // Tool calls
                    if let Some(tool_calls) = delta.get("tool_calls").and_then(|t| t.as_array()) {
                        for tc in tool_calls {
                            let index =
                                tc.get("index").and_then(|i| i.as_u64()).unwrap_or(0) as usize;

                            // #1494：新 index 的第一个 delta 出现 → 上一个（更小的、
                            // 未发出的）index 的 arguments 已完整——立即尝试解析并发出
                            // ToolCallCompleted，runtime 据此边流边执行。
                            if !current_tool_calls.contains_key(&index) {
                                let prior_indices: Vec<usize> = current_tool_calls
                                    .keys()
                                    .copied()
                                    .filter(|prior| *prior < index)
                                    .collect();
                                for prior_index in prior_indices {
                                    if completed_tool_indices.contains(&prior_index) {
                                        continue;
                                    }
                                    if let Some(accum) = current_tool_calls.get(&prior_index) {
                                        // 发出前必须补齐跨层 id，否则空 id 会让
                                        // runtime 无法把结果关联回原 tool call。
                                        let mut emitted = accum.clone();
                                        emitted.ensure_id(prior_index);
                                        if try_emit_completed_tool_call(
                                            handler,
                                            prior_index,
                                            emitted.id,
                                            emitted.name,
                                            &emitted.arguments,
                                        ) {
                                            completed_tool_indices.insert(prior_index);
                                        }
                                    }
                                }
                            }

                            // 获取或创建 tool call 条目
                            let entry = current_tool_calls.entry(index).or_default();

                            // 合并本帧 delta：id / name 的空串一律视为缺失，arguments 只追加。
                            let outcome = entry.merge(index, tc);

                            if outcome.name_is_new {
                                // 建卡前补齐跨层 id（provider 可能只发 name 不发 id）。
                                entry.ensure_id(index);
                                log::debug!(target: crate::LOG_TARGET,
                                    "[openai-compat stream] tool_use_start: name={} id={} index={}",
                                    entry.name, entry.id, index,
                                );
                                handler.emit_tool_use_start(&entry.name, Some(&entry.id), index);
                            }

                            if outcome.has_arguments && !entry.name.is_empty() {
                                if entry.delta_count == 1 {
                                    log::debug!(target: crate::LOG_TARGET,
                                        "[openai-compat stream] tool_args_first_delta: name={} index={} delta_bytes={}",
                                        entry.name, index, entry.arguments.len(),
                                    );
                                }
                                // Notify handler with accumulated arguments for
                                // real-time UI updates (e.g. showing file path).
                                entry.ensure_id(index);
                                handler.emit_tool_arguments_delta(
                                    index,
                                    &entry.name,
                                    Some(&entry.id),
                                    &entry.arguments,
                                );
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
    let current_reasoning = reasoning_normalizer.accumulated().to_string();
    let reasoning_stats = reasoning_normalizer.stats.clone();
    let total_chunks = chunk_index;
    if !current_reasoning.is_empty() {
        let reasoning_chars = current_reasoning.chars().count();
        log::debug!(target: crate::LOG_TARGET,
            "[openai-compat stream] reasoning summary: total_chunks={} \
             thinking_chars={} thinking_bytes={} \
             dedup={{none:{},snapshot_suffix:{},duplicate_drop:{},overlap_trim:{}}}",
            total_chunks, reasoning_chars, current_reasoning.len(),
            reasoning_stats.none, reasoning_stats.snapshot_suffix,
            reasoning_stats.duplicate_drop, reasoning_stats.overlap_trim,
        );
        content_blocks.push(ContentBlock::Thinking {
            thinking: current_reasoning,
            signature: None,
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
    for (tool_index, mut accum) in sorted_tool_calls {
        // content block 与跨层事件都携带该 id，缺失时按 index 兜底。
        accum.ensure_id(tool_index);
        let ToolCallAccum {
            id,
            name,
            arguments,
            delta_count,
        } = accum;
        if name.is_empty() {
            log::warn!(target: crate::LOG_TARGET,
                "[openai-compat stream] tool_call entry with empty name: id={}, args_bytes={}, delta_count={} — skipping",
                id, arguments.len(), delta_count
            );
            continue;
        }
        // Note: on_tool_use_start was already called during streaming
        // when the name first appeared in a delta chunk. No need to
        // call it again here.
        let input: serde_json::Value = if arguments.is_empty() {
            log::warn!(target: crate::LOG_TARGET,
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
                    if let Some(recovered) =
                        crate::adapters::json_recovery::try_complete_truncated_json(&arguments)
                    {
                        log::warn!(target: crate::LOG_TARGET,
                            "[openai-compat stream] tool_call '{}' (id={}) arguments truncated mid-string but heuristic recovery succeeded after {} delta chunks ({} bytes) — using recovered JSON. (Original error: {})",
                            name, id, delta_count, arguments.len(), e
                        );
                        recovered
                    } else {
                        let head: String = arguments.chars().take(300).collect();
                        let tail_rev: String =
                            arguments.chars().rev().take(200).collect::<String>();
                        let tail: String = tail_rev.chars().rev().collect();
                        log::warn!(target: crate::LOG_TARGET,
                            "[openai-compat stream] tool_call '{}' (id={}) arguments parse failed after {} delta chunks ({} bytes): {} — heuristic recovery also failed. head: {} | tail: {}",
                            name, id, delta_count, arguments.len(), e, head, tail
                        );
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
        content_blocks.push(ContentBlock::ToolUse {
            id: id.clone(),
            name: name.clone(),
            input: input.clone(),
        });
        // #1494 兜底：index 切换时未发出（参数解析失败 / 单 tool call 无切换）的，
        // 流结束解析成功后补发；已发出的跳过。
        if !completed_tool_indices.contains(&tool_index) {
            handler.emit_tool_call_completed(tool_index, id, name, input);
        }
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

    usage.finalize_total_tokens(0);

    Ok(StreamResponse {
        assistant_message: Message {
            role: Role::Assistant,
            content: content_blocks,
            metadata: None,
        },
        stop_reason,
    })
}

/// #1494：尝试把已完整的 arguments 解析为 JSON 并发出 `ToolCallCompleted`。
/// 解析失败（含 recovery 失败）时不发出——留到流结束兜底（那里有完整 recovery 逻辑）。
fn try_emit_completed_tool_call(
    handler: &mut dyn InvocationSink,
    index: usize,
    id: String,
    name: String,
    arguments: &str,
) -> bool {
    if arguments.trim().is_empty() {
        handler.emit_tool_call_completed(
            index,
            id,
            name,
            serde_json::Value::Object(serde_json::Map::new()),
        );
        return true;
    }
    let parsed = match serde_json::from_str::<serde_json::Value>(arguments) {
        Ok(value) => value,
        Err(error) => {
            match crate::adapters::json_recovery::try_complete_truncated_json(arguments) {
                Some(recovered) => recovered,
                None => {
                    log::debug!(target: crate::LOG_TARGET,
                        "[openai-compat stream] tool_call index={} name={} args not complete yet ({}) — deferring ToolCallCompleted to stream end",
                        index, name, error
                    );
                    return false;
                }
            }
        }
    };
    handler.emit_tool_call_completed(index, id, name, parsed);
    true
}

#[cfg(test)]
#[path = "stream_tests.rs"]
mod json_recovery_tests;

#[cfg(test)]
#[path = "tool_call_accum_tests.rs"]
mod tool_call_accum_tests;
