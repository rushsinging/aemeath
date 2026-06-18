use crate::business::chat::looping::events::{
    ChatEventSink, RuntimeStreamEvent, RuntimeToolCallStatus, RuntimeTurnContext,
};
use crate::business::chat::looping::tool_identity::ToolIdentityRegistry;
use provider::api::StreamHandler;
use crate::LOG_TARGET;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StreamingBlockKind {
    Text,
    Thinking,
}

/// Chat stream handler that forwards API streaming events to a runtime event sink.
pub struct RuntimeStreamHandler<S: ChatEventSink> {
    pub sink: S,
    pub first_text_time: Option<std::time::Instant>,
    pub total_chars: usize,
    pub last_tps_update: std::time::Instant,
    pub tool_identity: ToolIdentityRegistry,
    pub context: RuntimeTurnContext,
    active_streaming_block: Option<StreamingBlockKind>,
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
            active_streaming_block: None,
        }
    }

    pub fn runtime_tool_id(&self, index: usize, provider_id: Option<&str>) -> sdk::ids::ToolCallId {
        self.tool_identity.runtime_id_for_stream(index, provider_id)
    }

    fn begin_streaming_block(&mut self, kind: StreamingBlockKind) {
        if self
            .active_streaming_block
            .is_some_and(|active| active != kind)
        {
            self.complete_active_streaming_block();
        }
        self.active_streaming_block = Some(kind);
    }

    fn complete_active_streaming_block(&mut self) {
        if self.active_streaming_block.take().is_some() {
            self.sink.try_send_event(RuntimeStreamEvent::BlockComplete {
                context: self.context.clone(),
                text: String::new(),
            });
        }
    }
}

impl<S: ChatEventSink> StreamHandler for RuntimeStreamHandler<S> {
    fn on_text(&mut self, text: &str) {
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
        self.active_streaming_block = None;
        self.sink.try_send_event(RuntimeStreamEvent::BlockComplete {
            context: self.context.clone(),
            text: text.to_string(),
        });
    }

    fn on_thinking(&mut self, text: &str) {
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
