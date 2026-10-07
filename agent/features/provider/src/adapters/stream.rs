//! Stream parsing utilities for Anthropic API format.
//!
//! Internal decoders emit `InvocationDeltaData` events through the crate-private
//! [`InvocationSink`] trait; the [`InvocationEventHandler`] converts those
//! deltas into a pull-based `InvocationStreamData` of `InvocationEventData`s. The
//! decoder-side helper methods (`on_text`, `on_tool_use_start`, …) keep the
//! per-driver call sites unchanged while routing every emission through the
//! unified [`InvocationSink::on_delta`] entry point.

use super::constants::{
    ANTHROPIC_STREAM_IDLE_TIMEOUT, INVOCATION_STREAM_CAPACITY, STALL_THRESHOLD,
};
use crate::adapters::wire::*;
use crate::domain::capability::ReasoningLevel;
use crate::{
    InvocationDeltaData, InvocationEventData, InvocationStreamData, ProviderCompletionData,
    ProviderContentBlockData, ProviderError, ProviderErrorKind, ProviderStopReasonData,
    ProviderToolCallData, ProviderToolCallIdData, RawUsageSnapshotData,
};
use futures_util::StreamExt;
use reqwest::Response;
use share::message::{ContentBlock, Message, Role};
use std::io;
use tokio::io::AsyncBufReadExt;
use tokio_util::io::StreamReader;
use tokio_util::sync::CancellationToken;

/// Provider 内部 decoder 用来发射流式 delta 的内部接收器。
///
/// 此 trait **不**对外暴露——它只在 Provider crate 内部被 SSE/NDJSON decoder
/// 与 `InvocationStreamData` 构造器共享。Runtime/Context 与测试替身不得依赖。
/// 旧 sink 迁移桥已物理清零（#907）；本 trait 是 decoder 与
/// pull-based `InvocationStreamData` 之间的唯一内部契约。
pub(crate) trait InvocationSink: Send {
    /// 推入一个流式增量；Runtime 通过 `InvocationStreamData` 收到同样的事件。
    fn on_delta(&mut self, delta: InvocationDeltaData);

    /// 推入原始 SSE/NDJSON 行——仅供内部 usage 提取使用，不出现在
    /// `InvocationStreamData` 上。
    fn on_raw_line(&mut self, _line: &str) {}

    /// 流式中途诊断消息（idle timeout、retry/retry-able 等）；记录到 provider
    /// 日志，不作为 Runtime 事件。
    fn on_diagnostic(&mut self, message: &str) {
        log::warn!(target: crate::LOG_TARGET, "[provider stream] {}", message);
    }

    /// 流式 block 完成诊断；记录到 provider 调试日志。
    fn on_block_complete(&mut self, full_text: &str) {
        log::debug!(
            target: crate::LOG_TARGET,
            "[provider stream] block complete ({}B)",
            full_text.len()
        );
    }

    fn emit_text(&mut self, text: &str) {
        self.on_delta(InvocationDeltaData::Text(text.to_string()));
    }

    fn emit_thinking(&mut self, text: &str) {
        self.on_delta(InvocationDeltaData::Thinking {
            thinking: text.to_string(),
            signature: None,
        });
    }

    fn emit_tool_use_start(&mut self, name: &str, provider_id: Option<&str>, index: usize) {
        self.on_delta(InvocationDeltaData::ToolCallStarted {
            index,
            provider_id: provider_id.map(|id| ProviderToolCallIdData(id.to_string())),
            name: name.to_string(),
        });
    }

    fn emit_tool_arguments_delta(
        &mut self,
        index: usize,
        _name: &str,
        provider_id: Option<&str>,
        partial_args: &str,
    ) {
        self.on_delta(InvocationDeltaData::ToolArgumentsDelta {
            index,
            provider_id: provider_id.map(|id| ProviderToolCallIdData(id.to_string())),
            partial_json: partial_args.to_string(),
        });
    }

    /// #1494：tool call 参数完整（已验证 JSON）→ 立即发出，runtime 据此边流边执行。
    fn emit_tool_call_completed(
        &mut self,
        index: usize,
        id: String,
        name: String,
        arguments: serde_json::Value,
    ) {
        self.on_delta(InvocationDeltaData::ToolCallCompleted {
            index,
            call: ProviderToolCallData {
                id: ProviderToolCallIdData(id),
                name,
                arguments,
            },
        });
    }
}

#[derive(Clone, Copy)]
pub(crate) enum InvocationDecoder {
    Anthropic,
    OpenAiChat,
    OpenAiResponses,
    Ollama,
}

impl InvocationDecoder {
    fn raw_usage_from_line(self, line: &str) -> Option<RawUsageSnapshotData> {
        match self {
            Self::Anthropic => anthropic_raw_usage_from_line(line),
            Self::OpenAiChat => openai_chat_raw_usage_from_line(line),
            Self::OpenAiResponses => openai_responses_raw_usage_from_line(line),
            Self::Ollama => ollama_raw_usage_from_line(line),
        }
    }
}

fn json_payload(line: &str) -> Option<&str> {
    line.strip_prefix("data: ")
        .or_else(|| line.strip_prefix("data:"))
        .or(Some(line))
        .filter(|payload| !payload.is_empty() && *payload != "[DONE]")
}

fn anthropic_raw_usage_from_line(line: &str) -> Option<RawUsageSnapshotData> {
    let value: serde_json::Value = serde_json::from_str(json_payload(line)?).ok()?;
    let usage = match value.get("type").and_then(|kind| kind.as_str()) {
        Some("message_start") => value.get("message")?.get("usage")?,
        Some("message_delta") => value.get("usage")?,
        _ => return None,
    };
    Some(RawUsageSnapshotData {
        input_tokens: optional_u32(usage, "input_tokens"),
        output_tokens: optional_u32(usage, "output_tokens"),
        cache_read_tokens: optional_u32(usage, "cache_read_input_tokens"),
        cache_write_tokens: optional_u32(usage, "cache_creation_input_tokens"),
        reasoning_tokens: optional_u32(usage, "reasoning_tokens"),
    })
}

fn openai_chat_raw_usage_from_line(line: &str) -> Option<RawUsageSnapshotData> {
    let value: serde_json::Value = serde_json::from_str(json_payload(line)?).ok()?;
    value
        .get("usage")
        .filter(|usage| !usage.is_null())
        .map(crate::adapters::openai_compatible::parse_chat_raw_usage)
}

fn openai_responses_raw_usage_from_line(line: &str) -> Option<RawUsageSnapshotData> {
    let value: serde_json::Value = serde_json::from_str(json_payload(line)?).ok()?;
    (value.get("type").and_then(|kind| kind.as_str()) == Some("response.completed"))
        .then(|| value.get("response")?.get("usage"))
        .flatten()
        .map(crate::adapters::openai_compatible::parse_responses_raw_usage)
}

fn ollama_raw_usage_from_line(line: &str) -> Option<RawUsageSnapshotData> {
    let value: serde_json::Value = serde_json::from_str(line).ok()?;
    Some(RawUsageSnapshotData {
        input_tokens: optional_u32(&value, "prompt_eval_count"),
        output_tokens: optional_u32(&value, "eval_count"),
        cache_read_tokens: None,
        cache_write_tokens: None,
        reasoning_tokens: None,
    })
    .filter(RawUsageSnapshotData::was_reported)
}

fn optional_u32(value: &serde_json::Value, field: &str) -> Option<u32> {
    value
        .get(field)
        .and_then(serde_json::Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
}

pub(crate) fn parse_invocation_stream(
    response: Response,
    effective_reasoning: ReasoningLevel,
    cancel: CancellationToken,
) -> InvocationStreamData {
    invocation_stream_from_decoder(
        response,
        effective_reasoning,
        cancel,
        InvocationDecoder::Anthropic,
    )
}

#[cfg(test)]
fn observe_bridge_context(stage: &'static str) {
    bridge_context_observations()
        .lock()
        .expect("bridge context observations lock poisoned")
        .push((stage, logging::capture()));
}

#[cfg(test)]
fn bridge_context_observations(
) -> &'static std::sync::Mutex<Vec<(&'static str, logging::LogContext)>> {
    static OBSERVATIONS: std::sync::OnceLock<
        std::sync::Mutex<Vec<(&'static str, logging::LogContext)>>,
    > = std::sync::OnceLock::new();
    OBSERVATIONS.get_or_init(Default::default)
}

#[cfg(not(test))]
fn observe_bridge_context(_stage: &'static str) {}

pub(crate) fn invocation_stream_from_decoder(
    response: Response,
    effective_reasoning: ReasoningLevel,
    cancel: CancellationToken,
    decoder: InvocationDecoder,
) -> InvocationStreamData {
    let (sender, receiver) = std::sync::mpsc::sync_channel(INVOCATION_STREAM_CAPACITY);
    let runtime = tokio::runtime::Handle::current();
    let bridge_context = logging::capture();
    let producer_context = bridge_context.clone();
    let producer_runtime = runtime.clone();
    let producer_cancel = cancel.clone();
    tokio::task::spawn_blocking(move || {
        producer_runtime.block_on(logging::instrument(producer_context, async move {
            observe_bridge_context("producer");
            let usage = std::sync::Arc::new(std::sync::Mutex::new(RawUsageSnapshotData::default()));
            let mut handler = InvocationEventHandler::new(
                sender.clone(),
                producer_cancel.clone(),
                decoder,
                usage.clone(),
            );
            let result = match decoder {
                InvocationDecoder::Anthropic => {
                    parse_stream(response, &mut handler, &producer_cancel).await
                }
                InvocationDecoder::OpenAiChat => {
                    crate::adapters::openai_compatible::parse_openai_stream(
                        response,
                        &mut handler,
                        &producer_cancel,
                    )
                    .await
                }
                InvocationDecoder::OpenAiResponses => {
                    crate::adapters::openai_compatible::parse_responses_stream(
                        response,
                        &mut handler,
                        &producer_cancel,
                    )
                    .await
                }
                InvocationDecoder::Ollama => {
                    crate::adapters::ollama::stream::parse_ollama_stream(
                        response,
                        &mut handler,
                        &producer_cancel,
                    )
                    .await
                }
            };
            let terminal = match result {
                Ok(response) => InvocationEventData::Completed(completion_from_legacy(
                    response,
                    usage
                        .lock()
                        .expect("usage lock poisoned")
                        .clone()
                        .into_reported(),
                    effective_reasoning,
                )),
                Err(error) => InvocationEventData::Failed(provider_error_from_legacy(error)),
            };
            let _ = sender.send(terminal);
        }));
    });
    Box::pin(
        futures_util::stream::unfold(
            (receiver, runtime, bridge_context),
            |(receiver, runtime, bridge_context)| async move {
                let blocking_runtime = runtime.clone();
                let blocking_context = bridge_context.clone();
                tokio::task::spawn_blocking(move || {
                    blocking_runtime.block_on(logging::instrument(blocking_context, async move {
                        observe_bridge_context("consumer");
                        receiver.recv().ok().map(|event| (event, receiver))
                    }))
                })
                .await
                .ok()
                .flatten()
                .map(|(event, receiver)| (event, (receiver, runtime, bridge_context)))
            },
        )
        .fuse(),
    )
}

struct InvocationEventHandler {
    sender: std::sync::mpsc::SyncSender<InvocationEventData>,
    cancel: CancellationToken,
    decoder: InvocationDecoder,
    usage: std::sync::Arc<std::sync::Mutex<RawUsageSnapshotData>>,
}

impl InvocationEventHandler {
    fn new(
        sender: std::sync::mpsc::SyncSender<InvocationEventData>,
        cancel: CancellationToken,
        decoder: InvocationDecoder,
        usage: std::sync::Arc<std::sync::Mutex<RawUsageSnapshotData>>,
    ) -> Self {
        Self {
            sender,
            cancel,
            decoder,
            usage,
        }
    }

    fn send_delta(&self, delta: InvocationDeltaData) {
        observe_bridge_context("event");
        if self.sender.send(InvocationEventData::Delta(delta)).is_err() {
            self.cancel.cancel();
        }
    }
}

impl InvocationSink for InvocationEventHandler {
    fn on_delta(&mut self, delta: InvocationDeltaData) {
        self.send_delta(delta);
    }

    fn on_raw_line(&mut self, line: &str) {
        if let Some(latest) = self.decoder.raw_usage_from_line(line) {
            self.usage
                .lock()
                .expect("usage lock poisoned")
                .merge_reported(latest);
        }
    }
}

fn completion_from_legacy(
    response: StreamResponse,
    usage: Option<RawUsageSnapshotData>,
    effective_reasoning: ReasoningLevel,
) -> ProviderCompletionData {
    let output = response
        .assistant_message
        .content
        .into_iter()
        .filter_map(|block| match block {
            ContentBlock::Text { text } => Some(ProviderContentBlockData::Text(text)),
            ContentBlock::Thinking {
                thinking,
                signature,
            } => Some(ProviderContentBlockData::Thinking {
                thinking,
                signature,
            }),
            ContentBlock::ToolUse { id, name, input } => {
                Some(ProviderContentBlockData::ToolCall(ProviderToolCallData {
                    id: ProviderToolCallIdData(id),
                    name,
                    arguments: input,
                }))
            }
            ContentBlock::ToolResult { .. } | ContentBlock::Image { .. } => None,
        })
        .collect();
    ProviderCompletionData {
        output,
        stop_reason: response.stop_reason,
        usage,
        effective_reasoning,
    }
}

pub(crate) fn stream_read_error(error: io::Error) -> crate::LlmError {
    if error.kind() == io::ErrorKind::Other {
        crate::LlmError::StreamInterrupted(error.to_string())
    } else {
        crate::LlmError::Stream(error.to_string())
    }
}

/// Parse Anthropic-style SSE stream
pub async fn parse_stream(
    response: Response,
    handler: &mut dyn InvocationSink,
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
    let mut stop_reason = ProviderStopReasonData::EndTurn;

    let mut last_event_time: Option<std::time::Instant> = None;
    let mut tool_index: usize = 0;
    let mut current_signature: String = String::new();

    let byte_stream = response.bytes_stream().map(|r| r.map_err(io::Error::other));
    let reader = StreamReader::new(byte_stream);
    let mut lines = reader.lines();

    loop {
        // Calculate remaining idle timeout based on time since last event
        let idle_deadline = match last_event_time {
            Some(last) => last + ANTHROPIC_STREAM_IDLE_TIMEOUT,
            None => std::time::Instant::now() + ANTHROPIC_STREAM_IDLE_TIMEOUT,
        };
        let remaining = idle_deadline.saturating_duration_since(std::time::Instant::now());

        let line = tokio::select! {
            biased;
            _ = cancel.cancelled() => {
                return Err(crate::LlmError::Cancelled);
            }
            _ = tokio::time::sleep(remaining) => {
                handler.on_diagnostic(&format!("Stream idle timeout: no data for {}s", ANTHROPIC_STREAM_IDLE_TIMEOUT.as_secs()));
                return Err(crate::LlmError::Stream(format!(
                    "Stream idle timeout: no data received for {}s", ANTHROPIC_STREAM_IDLE_TIMEOUT.as_secs()
                )));
            }
            result = lines.next_line() => {
                match result.map_err(crate::adapters::stream::stream_read_error)? {
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
                        handler.emit_tool_use_start(&name, Some(&current_tool_id), tool_index);
                        tool_index += 1;
                    }
                    ContentBlockPayload::Thinking { thinking } => {
                        current_thinking = thinking.clone();
                        current_signature.clear();
                        if !thinking.is_empty() {
                            handler.emit_thinking(&thinking);
                        }
                    }
                    ContentBlockPayload::Unknown => {
                        // ignore unknown block types
                    }
                }
            }
            StreamEvent::ContentBlockDelta { delta, .. } => {
                match delta {
                    DeltaPayload::TextDelta { text } => {
                        handler.emit_text(&text);
                        current_text.push_str(&text);
                    }
                    DeltaPayload::InputJsonDelta { partial_json } => {
                        current_tool_json.push_str(&partial_json);
                        if !current_tool_name.is_empty() {
                            handler.emit_tool_arguments_delta(
                                tool_index.saturating_sub(1),
                                &current_tool_name,
                                Some(&current_tool_id),
                                &current_tool_json,
                            );
                        }
                    }
                    DeltaPayload::ThinkingDelta { thinking } => {
                        current_thinking.push_str(&thinking);
                        handler.emit_thinking(&thinking);
                    }
                    DeltaPayload::SignatureDelta { signature } => {
                        current_signature.push_str(&signature);
                    }
                    DeltaPayload::Unknown => {
                        // ignored
                    }
                }
            }
            StreamEvent::ContentBlockStop { .. } => {
                if !current_tool_id.is_empty() {
                    // 无参数工具（如 TaskListComplete、TaskList）不会产生
                    // InputJsonDelta，current_tool_json 为空字符串。此时应
                    // 视为空对象 {}，而非流截断错误。
                    let input: serde_json::Value = if current_tool_json.is_empty() {
                        serde_json::Value::Object(serde_json::Map::new())
                    } else {
                        match serde_json::from_str(&current_tool_json) {
                            Ok(v) => v,
                            Err(_) => {
                                // 截断恢复：尝试补全被切在字符串字面量中间的 arguments JSON。
                                if let Some(recovered) =
                                    crate::adapters::json_recovery::try_complete_truncated_json(
                                        &current_tool_json,
                                    )
                                {
                                    log::warn!(
                                        target: crate::LOG_TARGET,
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
                        }
                    };
                    // #1494：ContentBlockStop = Anthropic 协议级"参数完整"信号——
                    // 解析成功后立即发出 ToolCallCompleted，runtime 据此边流边执行。
                    handler.emit_tool_call_completed(
                        tool_index.saturating_sub(1),
                        current_tool_id.clone(),
                        current_tool_name.clone(),
                        input.clone(),
                    );
                    content_blocks.push(ContentBlock::ToolUse {
                        id: std::mem::take(&mut current_tool_id),
                        name: std::mem::take(&mut current_tool_name),
                        input,
                    });
                    current_tool_json.clear();
                } else if !current_thinking.is_empty() {
                    let signature = if current_signature.is_empty() {
                        None
                    } else {
                        Some(std::mem::take(&mut current_signature))
                    };
                    content_blocks.push(ContentBlock::Thinking {
                        thinking: std::mem::take(&mut current_thinking),
                        signature,
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
                    stop_reason = match reason.as_str() {
                        "end_turn" => ProviderStopReasonData::EndTurn,
                        "tool_use" => ProviderStopReasonData::ToolUse,
                        "max_tokens" => ProviderStopReasonData::MaxOutputTokens,
                        "stop_sequence" => ProviderStopReasonData::StopSequence,
                        other => ProviderStopReasonData::Other(other.to_string()),
                    };
                }
                if let Some(du) = delta_usage {
                    usage.output_tokens = du.output_tokens;
                }
            }
            StreamEvent::Error { error } => {
                handler.on_diagnostic(&error.message);
                return Err(crate::LlmError::Api {
                    error_type: error.error_type,
                    message: error.message,
                });
            }
            StreamEvent::MessageStop | StreamEvent::Ping => {}
        }
    }

    usage.finalize_anthropic_total_tokens();

    Ok(StreamResponse {
        assistant_message: Message {
            role: Role::Assistant,
            content: content_blocks,
            metadata: None,
        },
        stop_reason,
    })
}

#[cfg(test)]
#[path = "stream_usage_tests.rs"]
mod stream_usage_tests;

#[cfg(test)]
#[path = "stream_contract_tests.rs"]
mod contract_tests;

/// 流式路径的 LlmError→ProviderError：Network/StreamInterrupted/StreamTruncated
/// 标记 retryable（重试是安全的——请求未部分生效）；其余 fatal。
/// 与构造期映射（`From<LlmError>`，全 fatal）语义不同，勿合并。
fn provider_error_from_legacy(error: crate::LlmError) -> ProviderError {
    let retryable = matches!(
        &error,
        crate::LlmError::StreamInterrupted(_) | crate::LlmError::StreamTruncated { .. }
    );
    let kind = match &error {
        crate::LlmError::Cancelled => ProviderErrorKind::Cancelled,
        crate::LlmError::Api { .. } => ProviderErrorKind::UpstreamUnavailable,
        crate::LlmError::StreamInterrupted(_) | crate::LlmError::StreamTruncated { .. } => {
            ProviderErrorKind::StreamTruncated
        }
        crate::LlmError::Stream(_) => ProviderErrorKind::Protocol,
        crate::LlmError::Config(_) => ProviderErrorKind::Configuration,
    };
    let message = error.to_string();
    if retryable {
        ProviderError::retryable(kind, message)
    } else {
        ProviderError::fatal(kind, message)
    }
}

#[cfg(test)]
#[path = "stream_tests.rs"]
mod tests;
