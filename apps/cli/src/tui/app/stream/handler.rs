use crate::tui::app::UiEvent;
use aemeath_llm::provider::StreamHandler;
use tokio::sync::mpsc;

/// TUI stream handler that forwards API streaming events to the UI.
pub(crate) struct TuiStreamHandler {
    pub(crate) tx: mpsc::Sender<UiEvent>,
    pub(crate) first_text_time: Option<std::time::Instant>,
    pub(crate) total_chars: usize,
    pub(crate) last_tps_update: std::time::Instant,
}

impl StreamHandler for TuiStreamHandler {
    fn on_text(&mut self, text: &str) {
        if let Err(e) = self.tx.try_send(UiEvent::Text(text.to_string())) {
            log::warn!(
                "UI channel full, dropped Text event ({} bytes): {e}",
                text.len()
            );
        }
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
                    let _ = self.tx.try_send(UiEvent::LiveTps(tps));
                }
            }
        }
    }
    fn on_tool_use_start(&mut self, name: &str, index: usize) {
        if let Err(e) = self.tx.try_send(UiEvent::ToolCallStart {
            name: name.to_string(),
            index,
        }) {
            log::warn!("UI channel full, dropped ToolCallStart({name}[{index}]): {e}");
        }
    }
    fn on_error(&mut self, error: &str) {
        if let Err(e) = self
            .tx
            .try_send(UiEvent::SystemMessage(format!("[warn] {}", error)))
        {
            log::warn!("UI channel full, dropped SystemMessage: {e}");
        }
    }
    fn on_text_block_complete(&mut self, text: &str) {
        if let Err(e) = self
            .tx
            .try_send(UiEvent::TextBlockComplete(text.to_string()))
        {
            log::warn!(
                "UI channel full, dropped TextBlockComplete ({} bytes): {e}",
                text.len()
            );
        }
    }
    fn on_thinking(&mut self, text: &str) {
        if let Err(e) = self.tx.try_send(UiEvent::Thinking(text.to_string())) {
            log::warn!(
                "UI channel full, dropped Thinking event ({} bytes): {e}",
                text.len()
            );
        }
    }
    fn on_tool_arguments_delta(&mut self, index: usize, name: &str, partial_args: &str) {
        if let Err(e) = self.tx.try_send(UiEvent::ToolArgumentsDelta {
            index,
            name: name.to_string(),
            partial_args: partial_args.to_string(),
        }) {
            log::warn!("UI channel full, dropped ToolArgumentsDelta({name}[{index}]): {e}");
        }
    }
}
