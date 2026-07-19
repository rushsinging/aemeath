use crate::application::chat::looping::events::{
    ChatEventSink, RuntimeStreamEvent, RuntimeToolCallStatus, RuntimeTurnContext,
};
use crate::application::tool_coordination::identity::ToolIdentityRegistry;
use provider::{InvocationDelta, InvocationEvent};
use share::message::{ContentBlock, Message, Role};
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StreamingBlockKind {
    Text,
    Thinking,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StreamProgressSnapshot {
    pub first_visible_event_seen: bool,
    pub visible_progress_version: u64,
    pub phase: &'static str,
}

#[derive(Debug, Default)]
pub struct StreamProgressState {
    first_visible_event_seen: bool,
    visible_progress_version: u64,
    active_streaming_block: Option<StreamingBlockKind>,
}

impl StreamProgressState {
    pub fn snapshot(&self) -> StreamProgressSnapshot {
        StreamProgressSnapshot {
            first_visible_event_seen: self.first_visible_event_seen,
            visible_progress_version: self.visible_progress_version,
            phase: match self.active_streaming_block {
                Some(StreamingBlockKind::Text) => "writing",
                Some(StreamingBlockKind::Thinking) => "thinking",
                None if self.first_visible_event_seen => "waiting_model_output",
                None => "waiting_model_response",
            },
        }
    }
}

pub fn should_emit_model_stream_waiting(
    previous_visible_progress_version: Option<u64>,
    snapshot: &StreamProgressSnapshot,
) -> bool {
    previous_visible_progress_version
        .is_none_or(|previous| previous == snapshot.visible_progress_version)
}

pub struct InvocationEventReducer<S: ChatEventSink> {
    handler: RuntimeEventProjector<S>,
    saw_visible_delta: bool,
}

impl<S: ChatEventSink> InvocationEventReducer<S> {
    pub fn new(sink: S) -> Self {
        Self {
            handler: RuntimeEventProjector::new(sink),
            saw_visible_delta: false,
        }
    }

    pub fn with_tool_identity(
        sink: S,
        tool_identity: ToolIdentityRegistry,
        context: RuntimeTurnContext,
    ) -> Self {
        Self {
            handler: RuntimeEventProjector::with_tool_identity(sink, tool_identity, context),
            saw_visible_delta: false,
        }
    }

    pub fn progress_handle(&self) -> Arc<Mutex<StreamProgressState>> {
        self.handler.progress_handle()
    }

    pub fn apply(
        &mut self,
        event: InvocationEvent,
    ) -> Result<Option<provider::StreamResponse>, provider::ProviderError> {
        match event {
            InvocationEvent::Delta(delta) => {
                match delta {
                    InvocationDelta::Text(text) => {
                        self.saw_visible_delta = true;
                        self.handler.on_text(&text)
                    }
                    InvocationDelta::Thinking { thinking, .. } => {
                        self.saw_visible_delta = true;
                        self.handler.on_thinking(&thinking)
                    }
                    InvocationDelta::ToolCallStarted {
                        index,
                        provider_id,
                        name,
                    } => {
                        self.saw_visible_delta = true;
                        self.handler.on_tool_use_start(
                            &name,
                            provider_id.as_ref().map(|id| id.0.as_str()),
                            index,
                        )
                    }
                    InvocationDelta::ToolArgumentsDelta {
                        index,
                        provider_id,
                        partial_json,
                    } => {
                        self.saw_visible_delta = true;
                        self.handler.on_tool_arguments_delta(
                            index,
                            "",
                            provider_id.as_ref().map(|id| id.0.as_str()),
                            &partial_json,
                        )
                    }
                    InvocationDelta::ToolCallCompleted { .. }
                    | InvocationDelta::UsageSnapshot(_) => {}
                }
                Ok(None)
            }
            InvocationEvent::Completed(completion) => {
                self.handler.complete_active_streaming_block();
                if !self.saw_visible_delta {
                    for block in &completion.output {
                        match block {
                            provider::ProviderContentBlock::Text(text) => {
                                self.handler.on_text(text)
                            }
                            provider::ProviderContentBlock::Thinking { thinking, .. } => {
                                self.handler.on_thinking(thinking)
                            }
                            provider::ProviderContentBlock::ToolCall(call) => self
                                .handler
                                .on_tool_use_start(&call.name, Some(&call.id.0), 0),
                        }
                    }
                    self.handler.complete_active_streaming_block();
                }
                let content = completion
                    .output
                    .into_iter()
                    .map(|block| match block {
                        provider::ProviderContentBlock::Text(text) => ContentBlock::Text { text },
                        provider::ProviderContentBlock::Thinking {
                            thinking,
                            signature,
                        } => ContentBlock::Thinking {
                            thinking,
                            signature,
                        },
                        provider::ProviderContentBlock::ToolCall(call) => ContentBlock::ToolUse {
                            id: call.id.0,
                            name: call.name,
                            input: call.arguments,
                        },
                    })
                    .collect();
                let usage = completion.usage.unwrap_or_default();
                Ok(Some(provider::StreamResponse {
                    assistant_message: Message {
                        role: Role::Assistant,
                        content,
                        metadata: None,
                    },
                    usage: provider::Usage {
                        input_tokens: usage.input_tokens.unwrap_or(0),
                        output_tokens: usage.output_tokens.unwrap_or(0),
                        cached_tokens: usage.cache_read_tokens,
                        cache_creation_tokens: usage.cache_write_tokens,
                        reasoning_tokens: usage.reasoning_tokens,
                        total_tokens: usage
                            .input_tokens
                            .zip(usage.output_tokens)
                            .map(|(input, output)| input.saturating_add(output)),
                    },
                    stop_reason: match completion.stop_reason {
                        provider::ProviderStopReason::EndTurn => provider::StopReason::EndTurn,
                        provider::ProviderStopReason::ToolUse => provider::StopReason::ToolUse,
                        provider::ProviderStopReason::MaxOutputTokens => {
                            provider::StopReason::MaxTokens
                        }
                        provider::ProviderStopReason::ContentFiltered
                        | provider::ProviderStopReason::StopSequence
                        | provider::ProviderStopReason::Other(_) => provider::StopReason::EndTurn,
                    },
                }))
            }
            InvocationEvent::Failed(error) => {
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
    pub context: RuntimeTurnContext,
    progress: Arc<Mutex<StreamProgressState>>,
}

impl<S: ChatEventSink> RuntimeEventProjector<S> {
    pub fn new(sink: S) -> Self {
        Self::with_tool_identity(
            sink,
            ToolIdentityRegistry::new(),
            RuntimeTurnContext::new(sdk::ids::ChatId::new_v7(), sdk::ids::ChatTurnId::new_v7()),
        )
    }

    pub fn with_tool_identity(
        sink: S,
        tool_identity: ToolIdentityRegistry,
        context: RuntimeTurnContext,
    ) -> Self {
        Self {
            sink,
            first_text_time: None,
            total_chars: 0,
            last_tps_update: std::time::Instant::now(),
            tool_identity,
            context,
            progress: Arc::new(Mutex::new(StreamProgressState::default())),
        }
    }

    pub fn progress_handle(&self) -> Arc<Mutex<StreamProgressState>> {
        self.progress.clone()
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
            progress.visible_progress_version = progress.visible_progress_version.wrapping_add(1);
            first
        };
        if first {
            log::debug!(target: crate::LOG_TARGET,
                "model stream first visible event: kind={} {} turn_id={}",
                kind,
                detail(),
                self.context.turn_id,
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
        self.sink.try_send_event(RuntimeStreamEvent::Text {
            context: self.context.clone(),
            text: text.to_string(),
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
            "on_tool_use_start: name={} provider_id={:?} index={} turn_id={}",
            name, provider_id, index, self.context.turn_id,
        );
        self.complete_active_streaming_block();
        let id = self.runtime_tool_id(index, provider_id);
        self.sink.try_send_event(RuntimeStreamEvent::ToolCallStart {
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
        self.sink.try_send_event(RuntimeStreamEvent::Thinking {
            context: self.context.clone(),
            text: text.to_string(),
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
            .try_send_event(RuntimeStreamEvent::ToolCallUpdate {
                context: self.context.clone(),
                id,
                provider_id: provider_id.map(str::to_string),
                name: name.to_string(),
                index,
                arguments_delta: Some(partial_args.to_string()),
                arguments: None,
                status: RuntimeToolCallStatus::PendingArgs,
            });
    }
}

#[cfg(test)]
mod invocation_reducer_tests {
    use super::*;
    use crate::application::chat::looping::events::EventFuture;
    use provider::{
        InvocationDelta, InvocationEvent, ProviderCompletion, ProviderContentBlock, ProviderError,
        ProviderStopReason, RawUsageSnapshot, ReasoningLevel,
    };

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
    fn reducer_projects_text_and_builds_completed_response() {
        let sink = RecordingSink::default();
        let events = sink.0.clone();
        let mut reducer = InvocationEventReducer::new(sink);
        assert!(reducer
            .apply(InvocationEvent::Delta(InvocationDelta::Text(
                "hi".to_string()
            )))
            .unwrap()
            .is_none());
        let completion = ProviderCompletion {
            output: vec![ProviderContentBlock::Text("hi".to_string())],
            stop_reason: ProviderStopReason::EndTurn,
            usage: Some(RawUsageSnapshot {
                input_tokens: Some(2),
                output_tokens: Some(1),
                ..Default::default()
            }),
            effective_reasoning: ReasoningLevel::Off,
        };
        let response = reducer
            .apply(InvocationEvent::Completed(completion))
            .unwrap()
            .expect("completion produces response");
        assert_eq!(response.assistant_message.text_content(), "hi");
        assert_eq!(response.usage.input_tokens, 2);
        assert!(matches!(
            events.lock().unwrap().first(),
            Some(RuntimeStreamEvent::Text { text, .. }) if text == "hi"
        ));
    }

    #[test]
    fn reducer_maps_failed_terminal_to_error() {
        let sink = RecordingSink::default();
        let mut reducer = InvocationEventReducer::new(sink);
        let error = reducer
            .apply(InvocationEvent::Failed(ProviderError::cancelled()))
            .expect_err("failed terminal remains failure");
        assert!(error.is_cancelled());
    }
}
