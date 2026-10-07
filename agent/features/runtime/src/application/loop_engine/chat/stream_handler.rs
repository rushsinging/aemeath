use crate::application::loop_engine::chat::events::{
    ChatEventSink, RuntimeRunContext, RuntimeStreamEvent,
};
use crate::application::tool::coordination::identity::ToolIdentityRegistry;
use crate::ports::TokenUsageData;
use provider::{ProviderContentData, ProviderResponseChunk, ProviderStopReasonData};
use share::message::{ContentBlock, Message, Role};
use std::sync::{Arc, Mutex};

/// Runtime-facing aggregated invocation result。
///
/// Built by [`InvocationEventReducer`] from the `Stop` terminal frame, after
/// accumulating `Content`/`Usage` frames of one provider attempt.
#[derive(Debug)]
pub struct InvocationResponse {
    /// Assistant message assembled from the aggregated content frames.
    pub assistant_message: Message,
    /// Token usage snapshot from the `Usage` frames (optional fields, `None` = unreported).
    pub usage: TokenUsageData,
    /// Stop reason reported by the provider `Stop` frame.
    pub stop_reason: ProviderStopReasonData,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StreamingBlockKind {
    Text,
    Thinking,
}

#[derive(Debug, Default)]
pub struct StreamProgressState {
    first_visible_event_seen: bool,
    active_streaming_block: Option<StreamingBlockKind>,
}

/// Reduces one provider attempt's `ProviderResponseChunk` stream into an
/// [`InvocationResponse`] (local aggregation to the `Stop` frame).
///
/// #1880 v3 帧语义等价性（原 `Completed(completion)` 拆帧）：
/// - `Content(Text/Thinking)`：既是流增量也是完整块——即时投影（原 `Delta`
///   路径），同时聚合进 `output`（原 `completion.output`）。后到的同文本
///   完整 Thinking 帧（含 signature）覆盖先前累计块、不再投影，避免 UI 重复。
/// - `Content(ToolCall)`：终态完整块，仅聚合——回放由 `Stop` 帧的
///   `saw_visible_delta` 逻辑统一处理（等价原 Completed.output 中的块）。
/// - `Content(ToolCallStarted/ToolArgumentsDelta/ToolCallCompleted)`：
///   等价原 `ProviderContentData` 增量帧；`ToolCallCompleted` 携带的完整调用
///   同时聚合进 `output`（原 completion.output 的 ToolUse 块来源）。
/// - `Usage` → 原 `completion.usage`；`Stop` → 原 Completed 终态处理
///   （空输出校验、无 delta 回放、组装 InvocationResponse）。
/// - `Error` → 原 `Failed`。
pub struct InvocationEventReducer<S: ChatEventSink> {
    handler: RuntimeEventProjector<S>,
    saw_visible_delta: bool,
    /// 聚合中的终态内容块（原 `completion.output` 语义；只存 Text/Thinking/ToolCall）。
    output: Vec<ProviderContentData>,
    /// `Usage` 帧聚合（原 `completion.usage` 语义）。
    usage: TokenUsageData,
}

fn has_actionable_output(output: &[ProviderContentData]) -> bool {
    output.iter().any(|block| match block {
        ProviderContentData::Text(text) => !text.trim().is_empty(),
        ProviderContentData::ToolCall { .. } => true,
        ProviderContentData::Thinking { .. } => false,
        // 增量帧不进聚合 output（见 InvocationEventReducer 文档）。
        ProviderContentData::ToolCallStarted { .. }
        | ProviderContentData::ToolArgumentsDelta { .. }
        | ProviderContentData::ToolCallCompleted { .. } => false,
    })
}

fn empty_completion_error() -> provider::ProviderError {
    provider::ProviderError::retryable(
        provider::ProviderErrorKind::Protocol,
        "provider completed without assistant text or tool call",
    )
}

impl<S: ChatEventSink> InvocationEventReducer<S> {
    pub fn new(sink: S) -> Self {
        Self {
            handler: RuntimeEventProjector::new(sink),
            saw_visible_delta: false,
            output: Vec::new(),
            usage: TokenUsageData::default(),
        }
    }

    pub fn with_tool_identity(
        sink: S,
        tool_identity: ToolIdentityRegistry,
        context: RuntimeRunContext,
    ) -> Self {
        Self {
            handler: RuntimeEventProjector::with_tool_identity(sink, tool_identity, context),
            saw_visible_delta: false,
            output: Vec::new(),
            usage: TokenUsageData::default(),
        }
    }

    /// #1494：挂载边流边执行句柄（流中 `ToolCallCompleted` → 立即执行）。
    pub fn with_streaming_tool(
        mut self,
        streaming_tool: Arc<
            dyn crate::application::loop_engine::chat::streaming_tool::StreamingToolSubmitPort,
        >,
    ) -> Self {
        self.handler.streaming_tool = Some(streaming_tool);
        self
    }

    /// 聚合一个终态内容块进 `output`：相邻 Text/Thinking 增量合并为一块
    /// （等价原 completion.output 中 provider 已合并的块）。
    fn push_output(&mut self, block: ProviderContentData) {
        match block {
            ProviderContentData::Text(text) => {
                if let Some(ProviderContentData::Text(existing)) = self.output.last_mut() {
                    existing.push_str(&text);
                } else {
                    self.output.push(ProviderContentData::Text(text));
                }
            }
            ProviderContentData::Thinking {
                thinking,
                signature,
            } => {
                // 尾部完整帧覆盖同文本累计块（provider 聚合契约）。
                if let Some(slot) = self.output.iter_mut().find(|existing| {
                    matches!(
                        existing,
                        ProviderContentData::Thinking { thinking: t, .. } if *t == thinking
                    )
                }) {
                    *slot = ProviderContentData::Thinking {
                        thinking,
                        signature,
                    };
                } else if let Some(ProviderContentData::Thinking {
                    thinking: existing, ..
                }) = self.output.last_mut()
                {
                    existing.push_str(&thinking);
                } else {
                    self.output.push(ProviderContentData::Thinking {
                        thinking,
                        signature,
                    });
                }
            }
            ProviderContentData::ToolCall { .. } => self.output.push(block),
            // 增量帧不进终态输出（与 provider 非流式聚合同源）。
            ProviderContentData::ToolCallStarted { .. }
            | ProviderContentData::ToolArgumentsDelta { .. }
            | ProviderContentData::ToolCallCompleted { .. } => {}
        }
    }

    /// `Thinking` 帧是否为尾部完整帧（同文本覆盖已累计块）——此时不再投影。
    fn is_thinking_cover_frame(thinking: &str, output: &[ProviderContentData]) -> bool {
        output.iter().any(|existing| {
            matches!(
                existing,
                ProviderContentData::Thinking { thinking: t, .. } if t == thinking
            )
        })
    }

    pub fn apply(
        &mut self,
        chunk: ProviderResponseChunk,
    ) -> Result<Option<InvocationResponse>, provider::ProviderError> {
        match chunk {
            ProviderResponseChunk::Content(content) => {
                match content {
                    ProviderContentData::Text(text) => {
                        self.saw_visible_delta = true;
                        self.handler.on_text(&text);
                        self.push_output(ProviderContentData::Text(text));
                    }
                    ProviderContentData::Thinking {
                        thinking,
                        signature,
                    } => {
                        if Self::is_thinking_cover_frame(&thinking, &self.output) {
                            // 尾部完整帧：覆盖累计块即可，投影已在增量帧发生。
                            self.push_output(ProviderContentData::Thinking {
                                thinking,
                                signature,
                            });
                        } else {
                            self.saw_visible_delta = true;
                            self.handler.on_thinking(&thinking);
                            self.push_output(ProviderContentData::Thinking {
                                thinking,
                                signature,
                            });
                        }
                    }
                    ProviderContentData::ToolCall {
                        id,
                        name,
                        arguments,
                    } => {
                        // 终态完整块：仅聚合（等价原 completion.output 中的块）。
                        self.push_output(ProviderContentData::ToolCall {
                            id,
                            name,
                            arguments,
                        });
                    }
                    ProviderContentData::ToolCallStarted {
                        index,
                        provider_id,
                        name,
                    } => {
                        self.saw_visible_delta = true;
                        self.handler.on_tool_use_start(
                            &name,
                            provider_id.as_ref().map(|id| id.as_str()),
                            index,
                        );
                    }
                    ProviderContentData::ToolArgumentsDelta {
                        index,
                        provider_id,
                        partial_json,
                    } => {
                        self.saw_visible_delta = true;
                        self.handler.on_tool_arguments_delta(
                            index,
                            "",
                            provider_id.as_ref().map(|id| id.as_str()),
                            &partial_json,
                        );
                    }
                    ProviderContentData::ToolCallCompleted {
                        index,
                        id,
                        name,
                        arguments,
                    } => {
                        self.saw_visible_delta = true;
                        self.handler
                            .on_tool_call_completed(index, &id, &name, &arguments);
                        // 完整调用进聚合输出（原 completion.output 的 ToolUse 块来源）。
                        self.push_output(ProviderContentData::ToolCall {
                            id,
                            name,
                            arguments,
                        });
                    }
                }
                Ok(None)
            }
            ProviderResponseChunk::Usage(usage) => {
                self.usage.merge_reported(usage);
                Ok(None)
            }
            ProviderResponseChunk::Stop(stop_reason) => {
                self.handler.complete_active_streaming_block();
                if !has_actionable_output(&self.output) {
                    return Err(empty_completion_error());
                }
                if !self.saw_visible_delta {
                    for block in &self.output {
                        match block {
                            ProviderContentData::Text(text) => self.handler.on_text(text),
                            ProviderContentData::Thinking { thinking, .. } => {
                                self.handler.on_thinking(thinking)
                            }
                            ProviderContentData::ToolCall { id, name, .. } => {
                                self.handler.on_tool_use_start(name, Some(id.as_str()), 0)
                            }
                            _ => {}
                        }
                    }
                    self.handler.complete_active_streaming_block();
                }
                let content = std::mem::take(&mut self.output)
                    .into_iter()
                    .map(|block| match block {
                        ProviderContentData::Text(text) => ContentBlock::Text { text },
                        ProviderContentData::Thinking {
                            thinking,
                            signature,
                        } => ContentBlock::Thinking {
                            thinking,
                            signature,
                        },
                        ProviderContentData::ToolCall {
                            id,
                            name,
                            arguments,
                        } => ContentBlock::ToolUse {
                            id,
                            name,
                            input: arguments,
                        },
                        _ => unreachable!("only terminal content blocks accumulate"),
                    })
                    .collect();
                let usage = std::mem::take(&mut self.usage);
                Ok(Some(InvocationResponse {
                    assistant_message: Message {
                        role: Role::Assistant,
                        content,
                        metadata: None,
                    },
                    usage,
                    stop_reason,
                }))
            }
            ProviderResponseChunk::Error(error) => {
                self.handler.complete_active_streaming_block();
                Err(error)
            }
        }
    }
}

/// Chat stream handler that forwards API streaming events to a runtime event sink.
struct RuntimeEventProjector<S: ChatEventSink> {
    pub sink: S,
    pub first_text_time: Option<std::time::Instant>,
    pub total_chars: usize,
    pub last_tps_update: std::time::Instant,
    pub tool_identity: ToolIdentityRegistry,
    pub context: RuntimeRunContext,
    progress: Arc<Mutex<StreamProgressState>>,
    /// #1494：边流边执行句柄；`Some` 时流中 `ToolCallCompleted` 立即触发执行。
    streaming_tool: Option<
        Arc<dyn crate::application::loop_engine::chat::streaming_tool::StreamingToolSubmitPort>,
    >,
}

impl<S: ChatEventSink> RuntimeEventProjector<S> {
    pub fn new(sink: S) -> Self {
        Self::with_tool_identity(
            sink,
            ToolIdentityRegistry::new(),
            RuntimeRunContext::new(sdk::ids::ChatId::new_v7(), sdk::ids::ChatRunId::new_v7()),
        )
    }

    pub fn with_tool_identity(
        sink: S,
        tool_identity: ToolIdentityRegistry,
        context: RuntimeRunContext,
    ) -> Self {
        Self {
            sink,
            first_text_time: None,
            total_chars: 0,
            last_tps_update: std::time::Instant::now(),
            tool_identity,
            context,
            progress: Arc::new(Mutex::new(StreamProgressState::default())),
            streaming_tool: None,
        }
    }

    pub fn runtime_tool_id(&self, index: usize, provider_id: Option<&str>) -> sdk::ids::ToolCallId {
        self.tool_identity.runtime_id_for_stream(index, provider_id)
    }

    fn begin_streaming_block(&mut self, kind: StreamingBlockKind) {
        let should_complete = {
            let mut progress = self.progress.lock().unwrap();
            let should_complete = progress
                .active_streaming_block
                .is_some_and(|active| active != kind);
            progress.active_streaming_block = Some(kind);
            should_complete
        };
        if should_complete {
            self.sink.try_send_event(RuntimeStreamEvent::BlockComplete {
                context: self.context.clone(),
                text: String::new(),
            });
        }
    }

    fn mark_visible_event(&mut self, kind: &str, detail: impl FnOnce() -> String) {
        let first = {
            let mut progress = self.progress.lock().unwrap();
            let first = !progress.first_visible_event_seen;
            progress.first_visible_event_seen = true;
            first
        };
        if first {
            log::debug!(target: crate::LOG_TARGET,
                "model stream first visible event: kind={} {} run_id={}",
                kind,
                detail(),
                self.context.run_id,
            );
        }
    }

    pub fn complete_active_streaming_block(&mut self) {
        let had_active = {
            let mut progress = self.progress.lock().unwrap();
            progress.active_streaming_block.take().is_some()
        };
        if had_active {
            self.sink.try_send_event(RuntimeStreamEvent::BlockComplete {
                context: self.context.clone(),
                text: String::new(),
            });
        }
    }
    fn on_text(&mut self, text: &str) {
        self.mark_visible_event("text", || format!("bytes={}", text.len()));
        self.begin_streaming_block(StreamingBlockKind::Text);
        self.sink
            .try_send_event(RuntimeStreamEvent::AssistantTextDelta {
                context: self.context.clone(),
                delta: text.to_string(),
            });
        let now = std::time::Instant::now();
        if self.first_text_time.is_none() {
            self.first_text_time = Some(now);
            self.last_tps_update = now;
        }
        self.total_chars += text.len();
        if now.duration_since(self.last_tps_update).as_millis() >= 200 {
            self.last_tps_update = now;
            if let Some(start) = self.first_text_time {
                let elapsed = now.duration_since(start).as_secs_f64();
                if elapsed > 0.0 {
                    let estimated_tokens = self.total_chars as f64 / 3.0;
                    let tps = estimated_tokens / elapsed;
                    self.sink.try_send_event(RuntimeStreamEvent::LiveTps(tps));
                }
            }
        }
    }

    fn on_tool_use_start(&mut self, name: &str, provider_id: Option<&str>, index: usize) {
        self.mark_visible_event("tool_use_start", || {
            format!(
                "name={} provider_id={:?} index={}",
                name, provider_id, index
            )
        });
        log::debug!(target: crate::LOG_TARGET,
            "on_tool_use_start: name={} provider_id={:?} index={} run_id={}",
            name, provider_id, index, self.context.run_id,
        );
        self.complete_active_streaming_block();
        let id = self.runtime_tool_id(index, provider_id);
        self.sink
            .try_send_event(RuntimeStreamEvent::ToolCallStarted {
                context: self.context.clone(),
                id,
                provider_id: provider_id.map(str::to_string),
                name: name.to_string(),
                index,
            });
    }
    fn on_thinking(&mut self, text: &str) {
        self.mark_visible_event("thinking", || format!("bytes={}", text.len()));
        self.begin_streaming_block(StreamingBlockKind::Thinking);
        self.sink.try_send_event(RuntimeStreamEvent::ThinkingDelta {
            context: self.context.clone(),
            delta: text.to_string(),
        });
    }

    fn on_tool_arguments_delta(
        &mut self,
        index: usize,
        name: &str,
        provider_id: Option<&str>,
        partial_args: &str,
    ) {
        self.mark_visible_event("tool_args", || {
            format!(
                "name={} provider_id={:?} index={} bytes={}",
                name,
                provider_id,
                index,
                partial_args.len()
            )
        });
        self.complete_active_streaming_block();
        let id = self.runtime_tool_id(index, provider_id);
        self.sink
            .try_send_event(RuntimeStreamEvent::ToolCallArgumentsDelta {
                context: self.context.clone(),
                id,
                provider_id: provider_id.map(str::to_string),
                name: name.to_string(),
                index,
                delta: partial_args.to_string(),
            });
    }

    /// #1494：provider 已给出完整验证过的 tool call → 立即旁路执行（边流边执行）。
    fn on_tool_call_completed(
        &mut self,
        index: usize,
        provider_id: &str,
        name: &str,
        arguments: &serde_json::Value,
    ) {
        let Some(executor) = &self.streaming_tool else {
            return;
        };
        let id = self.runtime_tool_id(index, Some(provider_id));
        executor.submit(crate::application::tool::agent::ToolCall {
            id,
            provider_id: provider_id.to_string(),
            name: name.to_string(),
            index,
            input: arguments.clone(),
        });
    }
}

#[cfg(test)]
mod invocation_reducer_tests {
    use super::*;
    use crate::application::loop_engine::chat::events::EventFuture;
    use provider::{ProviderError, ProviderStopReasonData};
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    struct RecordingSink(Arc<Mutex<Vec<RuntimeStreamEvent>>>);

    impl ChatEventSink for RecordingSink {
        fn send_event<'a>(&'a self, event: RuntimeStreamEvent) -> EventFuture<'a> {
            Box::pin(async move { self.0.lock().unwrap().push(event) })
        }

        fn try_send_event(&self, event: RuntimeStreamEvent) {
            self.0.lock().unwrap().push(event);
        }
    }

    #[test]
    fn reducer_projects_text_and_builds_stopped_response() {
        let sink = RecordingSink::default();
        let events = sink.0.clone();
        let mut reducer = InvocationEventReducer::new(sink);
        // v3：Content(Text) 单帧即增量亦即完整（尾帧不重发）。
        assert!(reducer
            .apply(ProviderResponseChunk::Content(ProviderContentData::Text(
                "hi".to_string()
            )))
            .unwrap()
            .is_none());
        assert!(reducer
            .apply(ProviderResponseChunk::Usage(TokenUsageData {
                input_tokens: Some(2),
                output_tokens: Some(1),
                ..Default::default()
            }))
            .unwrap()
            .is_none());
        let response = reducer
            .apply(ProviderResponseChunk::Stop(ProviderStopReasonData::EndTurn))
            .unwrap()
            .expect("stop frame produces response");
        assert_eq!(response.assistant_message.text_content(), "hi");
        assert_eq!(response.usage.input_tokens, Some(2));
        assert_eq!(response.stop_reason, ProviderStopReasonData::EndTurn);
        assert!(matches!(
            events.lock().unwrap().first(),
            Some(RuntimeStreamEvent::AssistantTextDelta { delta, .. }) if delta == "hi"
        ));
    }

    #[test]
    fn reducer_replays_aggregated_blocks_when_no_visible_delta() {
        let sink = RecordingSink::default();
        let events = sink.0.clone();
        let mut reducer = InvocationEventReducer::new(sink);
        // 终态完整 ToolCall 帧：仅聚合，不即时投影（等价原 Completed.output）。
        assert!(reducer
            .apply(ProviderResponseChunk::Content(
                ProviderContentData::ToolCall {
                    id: "toolu_1".to_string(),
                    name: "echo".to_string(),
                    arguments: serde_json::json!({"text": "hi"}),
                }
            ))
            .unwrap()
            .is_none());
        let response = reducer
            .apply(ProviderResponseChunk::Stop(ProviderStopReasonData::ToolUse))
            .unwrap()
            .expect("stop frame produces response");
        assert!(matches!(
            &response.assistant_message.content[..],
            [ContentBlock::ToolUse { id, name, .. }] if id == "toolu_1" && name == "echo"
        ));
        // 回放：Stop 帧把聚合块补投影为 ToolCallStarted。
        assert!(matches!(
            events.lock().unwrap().first(),
            Some(RuntimeStreamEvent::ToolCallStarted { name, .. }) if name == "echo"
        ));
    }

    #[test]
    fn reducer_thinking_cover_frame_does_not_reproject() {
        let sink = RecordingSink::default();
        let events = sink.0.clone();
        let mut reducer = InvocationEventReducer::new(sink);
        reducer
            .apply(ProviderResponseChunk::Content(
                ProviderContentData::Thinking {
                    thinking: "part".to_string(),
                    signature: None,
                },
            ))
            .unwrap();
        // 尾部完整帧：同文本 + signature → 覆盖累计块、不重复投影。
        reducer
            .apply(ProviderResponseChunk::Content(
                ProviderContentData::Thinking {
                    thinking: "part".to_string(),
                    signature: Some("sig".to_string()),
                },
            ))
            .unwrap();
        let response = reducer
            .apply(ProviderResponseChunk::Stop(ProviderStopReasonData::EndTurn))
            .expect_err("thinking-only output is not actionable");
        assert_eq!(response.kind, provider::ProviderErrorKind::Protocol);
        // 只投影一次 thinking delta。
        let thinking_count = events
            .lock()
            .unwrap()
            .iter()
            .filter(|event| matches!(event, RuntimeStreamEvent::ThinkingDelta { .. }))
            .count();
        assert_eq!(thinking_count, 1);
    }

    #[test]
    fn reducer_maps_error_terminal_to_error() {
        let sink = RecordingSink::default();
        let mut reducer = InvocationEventReducer::new(sink);
        let error = reducer
            .apply(ProviderResponseChunk::Error(ProviderError::cancelled()))
            .expect_err("error terminal remains failure");
        assert!(error.is_cancelled());
    }
}
