//! Stream parsing utilities for Anthropic API format.
//!
//! Internal decoders emit `InvocationDelta` events through the crate-private
//! [`InvocationSink`] trait; the [`InvocationEventHandler`] converts those
//! deltas into a pull-based `InvocationStream` of `InvocationEvent`s. The
//! decoder-side helper methods (`on_text`, `on_tool_use_start`, …) keep the
//! per-driver call sites unchanged while routing every emission through the
//! unified [`InvocationSink::on_delta`] entry point.

use crate::domain::invoke::*;
use crate::{
    InvocationDelta, InvocationEvent, InvocationStream, ProviderCompletion, ProviderContentBlock,
    ProviderError, ProviderErrorKind, ProviderStopReason, ProviderToolCall, ProviderToolCallId,
    RawUsageSnapshot, ReasoningLevel,
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
/// 与 `InvocationStream` 构造器共享。Runtime/Context 与测试替身不得依赖。
/// 旧 sink 迁移桥已物理清零（#907）；本 trait 是 decoder 与
/// pull-based `InvocationStream` 之间的唯一内部契约。
pub(crate) trait InvocationSink: Send {
    /// 推入一个流式增量；Runtime 通过 `InvocationStream` 收到同样的事件。
    fn on_delta(&mut self, delta: InvocationDelta);

    /// 推入原始 SSE/NDJSON 行——仅供内部 usage 提取使用，不出现在
    /// `InvocationStream` 上。
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
        self.on_delta(InvocationDelta::Text(text.to_string()));
    }

    fn emit_thinking(&mut self, text: &str) {
        self.on_delta(InvocationDelta::Thinking {
            thinking: text.to_string(),
            signature: None,
        });
    }

    fn emit_tool_use_start(&mut self, name: &str, provider_id: Option<&str>, index: usize) {
        self.on_delta(InvocationDelta::ToolCallStarted {
            index,
            provider_id: provider_id.map(|id| ProviderToolCallId(id.to_string())),
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
        self.on_delta(InvocationDelta::ToolArgumentsDelta {
            index,
            provider_id: provider_id.map(|id| ProviderToolCallId(id.to_string())),
            partial_json: partial_args.to_string(),
        });
    }
}

const INVOCATION_STREAM_CAPACITY: usize = 1;

#[derive(Clone, Copy)]
pub(crate) enum InvocationDecoder {
    Anthropic,
    OpenAiChat,
    OpenAiResponses,
    Ollama,
}

impl InvocationDecoder {
    fn raw_usage_from_line(self, line: &str) -> Option<RawUsageSnapshot> {
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

fn anthropic_raw_usage_from_line(line: &str) -> Option<RawUsageSnapshot> {
    let value: serde_json::Value = serde_json::from_str(json_payload(line)?).ok()?;
    let usage = match value.get("type").and_then(|kind| kind.as_str()) {
        Some("message_start") => value.get("message")?.get("usage")?,
        Some("message_delta") => value.get("usage")?,
        _ => return None,
    };
    Some(RawUsageSnapshot {
        input_tokens: optional_u32(usage, "input_tokens"),
        output_tokens: optional_u32(usage, "output_tokens"),
        cache_read_tokens: optional_u32(usage, "cache_read_input_tokens"),
        cache_write_tokens: optional_u32(usage, "cache_creation_input_tokens"),
        reasoning_tokens: optional_u32(usage, "reasoning_tokens"),
    })
}

fn openai_chat_raw_usage_from_line(line: &str) -> Option<RawUsageSnapshot> {
    let value: serde_json::Value = serde_json::from_str(json_payload(line)?).ok()?;
    value
        .get("usage")
        .filter(|usage| !usage.is_null())
        .map(crate::adapters::openai_compatible::parse_chat_raw_usage)
}

fn openai_responses_raw_usage_from_line(line: &str) -> Option<RawUsageSnapshot> {
    let value: serde_json::Value = serde_json::from_str(json_payload(line)?).ok()?;
    (value.get("type").and_then(|kind| kind.as_str()) == Some("response.completed"))
        .then(|| value.get("response")?.get("usage"))
        .flatten()
        .map(crate::adapters::openai_compatible::parse_responses_raw_usage)
}

fn ollama_raw_usage_from_line(line: &str) -> Option<RawUsageSnapshot> {
    let value: serde_json::Value = serde_json::from_str(line).ok()?;
    Some(RawUsageSnapshot {
        input_tokens: optional_u32(&value, "prompt_eval_count"),
        output_tokens: optional_u32(&value, "eval_count"),
        cache_read_tokens: None,
        cache_write_tokens: None,
        reasoning_tokens: None,
    })
    .filter(RawUsageSnapshot::was_reported)
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
) -> InvocationStream {
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
) -> InvocationStream {
    let (sender, receiver) = std::sync::mpsc::sync_channel(INVOCATION_STREAM_CAPACITY);
    let runtime = tokio::runtime::Handle::current();
    let bridge_context = logging::capture();
    let producer_context = bridge_context.clone();
    let producer_runtime = runtime.clone();
    let producer_cancel = cancel.clone();
    tokio::task::spawn_blocking(move || {
        producer_runtime.block_on(logging::instrument(producer_context, async move {
            observe_bridge_context("producer");
            let usage = std::sync::Arc::new(std::sync::Mutex::new(RawUsageSnapshot::default()));
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
                Ok(response) => InvocationEvent::Completed(completion_from_legacy(
                    response,
                    usage
                        .lock()
                        .expect("usage lock poisoned")
                        .clone()
                        .into_reported(),
                    effective_reasoning,
                )),
                Err(error) => InvocationEvent::Failed(provider_error_from_legacy(error)),
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
    sender: std::sync::mpsc::SyncSender<InvocationEvent>,
    cancel: CancellationToken,
    decoder: InvocationDecoder,
    usage: std::sync::Arc<std::sync::Mutex<RawUsageSnapshot>>,
}

impl InvocationEventHandler {
    fn new(
        sender: std::sync::mpsc::SyncSender<InvocationEvent>,
        cancel: CancellationToken,
        decoder: InvocationDecoder,
        usage: std::sync::Arc<std::sync::Mutex<RawUsageSnapshot>>,
    ) -> Self {
        Self {
            sender,
            cancel,
            decoder,
            usage,
        }
    }

    fn send_delta(&self, delta: InvocationDelta) {
        observe_bridge_context("event");
        if self.sender.send(InvocationEvent::Delta(delta)).is_err() {
            self.cancel.cancel();
        }
    }
}

impl InvocationSink for InvocationEventHandler {
    fn on_delta(&mut self, delta: InvocationDelta) {
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
    usage: Option<RawUsageSnapshot>,
    effective_reasoning: ReasoningLevel,
) -> ProviderCompletion {
    let output = response
        .assistant_message
        .content
        .into_iter()
        .filter_map(|block| match block {
            ContentBlock::Text { text } => Some(ProviderContentBlock::Text(text)),
            ContentBlock::Thinking {
                thinking,
                signature,
            } => Some(ProviderContentBlock::Thinking {
                thinking,
                signature,
            }),
            ContentBlock::ToolUse { id, name, input } => {
                Some(ProviderContentBlock::ToolCall(ProviderToolCall {
                    id: ProviderToolCallId(id),
                    name,
                    arguments: input,
                }))
            }
            ContentBlock::ToolResult { .. } | ContentBlock::Image { .. } => None,
        })
        .collect();
    ProviderCompletion {
        output,
        stop_reason: match response.stop_reason {
            StopReason::EndTurn => ProviderStopReason::EndTurn,
            StopReason::ToolUse => ProviderStopReason::ToolUse,
            StopReason::MaxTokens => ProviderStopReason::MaxOutputTokens,
        },
        usage,
        effective_reasoning,
    }
}

fn provider_error_from_legacy(error: crate::LlmError) -> ProviderError {
    let kind = match error {
        crate::LlmError::Cancelled => ProviderErrorKind::Cancelled,
        crate::LlmError::RateLimited => ProviderErrorKind::RateLimited,
        crate::LlmError::ContextTooLong => ProviderErrorKind::ContextTooLong,
        crate::LlmError::Network(_) => ProviderErrorKind::Network,
        crate::LlmError::Api { .. } => ProviderErrorKind::UpstreamUnavailable,
        crate::LlmError::StreamTruncated { .. } => ProviderErrorKind::StreamTruncated,
        crate::LlmError::Stream(_) => ProviderErrorKind::Protocol,
        crate::LlmError::Config(_) => ProviderErrorKind::Configuration,
    };
    ProviderError::fatal(kind, error.to_string())
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
    let mut stop_reason = StopReason::EndTurn;

    const STREAM_IDLE_TIMEOUT: std::time::Duration =
        std::time::Duration::from_secs(crate::ANTHROPIC_STREAM_IDLE_TIMEOUT_SECS);
    const STALL_THRESHOLD: std::time::Duration =
        std::time::Duration::from_secs(crate::STALL_THRESHOLD_SECS);
    let mut last_event_time: Option<std::time::Instant> = None;
    let mut tool_index: usize = 0;
    let mut current_signature: String = String::new();

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
                handler.on_diagnostic(&format!("Stream idle timeout: no data for {}s", STREAM_IDLE_TIMEOUT.as_secs()));
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
                    stop_reason = StopReason::parse(&reason);
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
#[path = "stream_contract_tests.rs"]
mod contract_tests;

#[cfg(test)]
mod tests {
    use super::{bridge_context_observations, invocation_stream_from_decoder, InvocationDecoder};
    use crate::domain::invoke::StreamEvent;
    use crate::ReasoningLevel;
    use futures_util::StreamExt;
    use tokio_util::sync::CancellationToken;

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn legacy_stream_bridge_preserves_each_callers_opaque_log_context() {
        bridge_context_observations()
            .lock()
            .expect("bridge context observations lock poisoned")
            .clear();

        let server = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind fixture server");
        let address = server.local_addr().expect("fixture server address");
        let fixture = concat!(
            "HTTP/1.1 200 OK\r\n",
            "content-type: text/event-stream\r\n",
            "connection: close\r\n\r\n",
            "data: {\"type\":\"content_block_delta\",\"index\":0,",
            "\"delta\":{\"type\":\"text_delta\",\"text\":\"x\"}}\n\n",
            "data: [DONE]\n\n"
        );
        let fixture_server = tokio::spawn(async move {
            for _ in 0..2 {
                let (mut socket, _) = server.accept().await.expect("accept fixture request");
                let mut request = [0_u8; 1024];
                let _ = tokio::io::AsyncReadExt::read(&mut socket, &mut request).await;
                tokio::io::AsyncWriteExt::write_all(&mut socket, fixture.as_bytes())
                    .await
                    .expect("write fixture response");
            }
        });

        let run = |request_id: &'static str| async move {
            let context = logging::LogContext {
                session_id: Some(format!("session-{request_id}")),
                request_id: Some(request_id.to_string()),
                ..logging::LogContext::default()
            };
            logging::instrument(context.clone(), async move {
                let response = reqwest::get(format!("http://{address}/{request_id}"))
                    .await
                    .expect("fixture response");
                let mut stream = invocation_stream_from_decoder(
                    response,
                    ReasoningLevel::Off,
                    CancellationToken::new(),
                    InvocationDecoder::Anthropic,
                );
                while stream.next().await.is_some() {}
                context
            })
            .await
        };

        let (first, second) = tokio::join!(run("request-a"), run("request-b"));
        fixture_server.await.expect("fixture server task");

        let observations = bridge_context_observations()
            .lock()
            .expect("bridge context observations lock poisoned")
            .clone();
        for expected in [first, second] {
            for stage in ["producer", "event", "consumer"] {
                assert!(
                    observations
                        .iter()
                        .any(|(observed_stage, context)| *observed_stage == stage
                            && context == &expected),
                    "missing {stage} observation for {expected:?}; got {observations:?}"
                );
            }
        }
    }

    #[test]
    fn anthropic_message_start_deserializes_all_input_token_components() {
        let event: StreamEvent = serde_json::from_value(serde_json::json!({
            "type": "message_start",
            "message": {
                "usage": {
                    "input_tokens": 100,
                    "cache_read_input_tokens": 80,
                    "cache_creation_input_tokens": 30,
                    "output_tokens": 0
                }
            }
        }))
        .expect("valid Anthropic message_start fixture");

        let StreamEvent::MessageStart { message } = event else {
            panic!("expected message_start");
        };
        assert_eq!(message.usage.input_tokens, 100);
        assert_eq!(message.usage.cached_tokens, Some(80));
        assert_eq!(message.usage.cache_creation_tokens, Some(30));
        assert_eq!(message.usage.normalized_total_tokens(110), 210);
    }
}
