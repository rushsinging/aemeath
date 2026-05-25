use crate::tui_loop::events::{RuntimeStreamEvent, TuiLoopEventSink};
use provider::StreamHandler;

/// TUI stream handler that forwards API streaming events to a runtime event sink.
pub struct RuntimeStreamHandler<S: TuiLoopEventSink> {
    pub sink: S,
    pub first_text_time: Option<std::time::Instant>,
    pub total_chars: usize,
    pub last_tps_update: std::time::Instant,
}

impl<S: TuiLoopEventSink> RuntimeStreamHandler<S> {
    pub fn new(sink: S) -> Self {
        Self {
            sink,
            first_text_time: None,
            total_chars: 0,
            last_tps_update: std::time::Instant::now(),
        }
    }
}

impl<S: TuiLoopEventSink> StreamHandler for RuntimeStreamHandler<S> {
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

    fn on_tool_use_start(&mut self, name: &str, index: usize) {
        self.sink.try_send_event(RuntimeStreamEvent::ToolCallStart {
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

    fn on_tool_arguments_delta(&mut self, index: usize, name: &str, partial_args: &str) {
        self.sink
            .try_send_event(RuntimeStreamEvent::ToolArgumentsDelta {
                index,
                name: name.to_string(),
                partial_args: partial_args.to_string(),
            });
    }
}
