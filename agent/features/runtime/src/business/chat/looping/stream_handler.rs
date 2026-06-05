use crate::business::chat::looping::events::{ChatEventSink, RuntimeStreamEvent};
use provider::api::StreamHandler;
use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

/// Chat stream handler that forwards API streaming events to a runtime event sink.
pub struct RuntimeStreamHandler<S: ChatEventSink> {
    pub sink: S,
    pub first_text_time: Option<std::time::Instant>,
    pub total_chars: usize,
    pub last_tps_update: std::time::Instant,
    pub next_tool_id: Arc<AtomicUsize>,
    pub stream_tool_ids: HashMap<usize, String>,
}

impl<S: ChatEventSink> RuntimeStreamHandler<S> {
    pub fn new(sink: S) -> Self {
        Self::with_tool_id_counter(sink, Arc::new(AtomicUsize::new(0)))
    }

    pub fn with_tool_id_counter(sink: S, next_tool_id: Arc<AtomicUsize>) -> Self {
        Self {
            sink,
            first_text_time: None,
            total_chars: 0,
            last_tps_update: std::time::Instant::now(),
            next_tool_id,
            stream_tool_ids: HashMap::new(),
        }
    }

    pub fn runtime_tool_id(&mut self, index: usize) -> String {
        self.stream_tool_ids
            .entry(index)
            .or_insert_with(|| {
                let next = self.next_tool_id.fetch_add(1, Ordering::Relaxed) + 1;
                format!("tool-{next}")
            })
            .clone()
    }
}

impl<S: ChatEventSink> StreamHandler for RuntimeStreamHandler<S> {
    fn on_text(&mut self, text: &str) {
        self.sink
            .try_send_event(RuntimeStreamEvent::Text(text.to_string()));
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
        let id = self.runtime_tool_id(index);
        self.sink.try_send_event(RuntimeStreamEvent::ToolCallStart {
            id,
            provider_id: provider_id.map(str::to_string),
            name: name.to_string(),
            index,
        });
    }
    fn on_error(&mut self, error: &str) {
        self.sink
            .try_send_event(RuntimeStreamEvent::SystemMessage(format!(
                "[warn] {}",
                error
            )));
    }

    fn on_text_block_complete(&mut self, text: &str) {
        self.sink
            .try_send_event(RuntimeStreamEvent::TextBlockComplete(text.to_string()));
    }

    fn on_thinking(&mut self, text: &str) {
        self.sink
            .try_send_event(RuntimeStreamEvent::Thinking(text.to_string()));
    }

    fn on_tool_arguments_delta(
        &mut self,
        index: usize,
        name: &str,
        provider_id: Option<&str>,
        partial_args: &str,
    ) {
        let id = self.runtime_tool_id(index);
        self.sink
            .try_send_event(RuntimeStreamEvent::ToolArgumentsDelta {
                id,
                provider_id: provider_id.map(str::to_string),
                index,
                name: name.to_string(),
                partial_args: partial_args.to_string(),
            });
    }
}
