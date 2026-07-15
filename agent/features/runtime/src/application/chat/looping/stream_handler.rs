use crate::application::chat::looping::events::{
    ChatEventSink, RuntimeStreamEvent, RuntimeToolCallStatus, RuntimeTurnContext,
};
use crate::application::chat::looping::tool_identity::ToolIdentityRegistry;
use crate::LOG_TARGET;
use provider::api::StreamHandler;
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

/// Chat stream handler that forwards API streaming events to a runtime event sink.
pub struct RuntimeStreamHandler<S: ChatEventSink> {
    pub sink: S,
    pub first_text_time: Option<std::time::Instant>,
    pub total_chars: usize,
    pub last_tps_update: std::time::Instant,
    pub tool_identity: ToolIdentityRegistry,
    pub context: RuntimeTurnContext,
    progress: Arc<Mutex<StreamProgressState>>,
}

impl<S: ChatEventSink> RuntimeStreamHandler<S> {
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
            log::debug!(target: LOG_TARGET,
                "model stream first visible event: kind={} {} turn_id={}",
                kind,
                detail(),
                self.context.turn_id,
            );
        }
    }

    pub fn progress_snapshot(&self) -> StreamProgressSnapshot {
        self.progress.lock().unwrap().snapshot()
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
}

impl<S: ChatEventSink> StreamHandler for RuntimeStreamHandler<S> {
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
        log::debug!(target: LOG_TARGET,
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
    fn on_error(&mut self, error: &str) {
        self.complete_active_streaming_block();
        self.sink
            .try_send_event(RuntimeStreamEvent::SystemMessage(format!(
                "[warn] {}",
                error
            )));
    }

    fn on_block_complete(&mut self, text: &str) {
        {
            let mut progress = self.progress.lock().unwrap();
            progress.active_streaming_block = None;
        }
        self.sink.try_send_event(RuntimeStreamEvent::BlockComplete {
            context: self.context.clone(),
            text: text.to_string(),
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
