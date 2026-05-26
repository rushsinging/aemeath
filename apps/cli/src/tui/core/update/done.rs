use crate::tui::core::{App, UiEvent};
use tokio::sync::mpsc;

pub(super) fn input_queue_preview(queue: &std::collections::VecDeque<String>) -> String {
    queue
        .front()
        .map(|msg| {
            let preview: String = msg.chars().take(80).collect();
            if msg.chars().count() > 80 {
                format!("{preview}…")
            } else {
                preview
            }
        })
        .unwrap_or_default()
}

impl App {
    pub(super) fn handle_done(
        &mut self,
        ui_tx: &mpsc::Sender<UiEvent>,
        elapsed: Option<std::time::Duration>,
    ) {
        if let Some(dur) = elapsed {
            self.output_area.push_done(dur);
        }
        self.output_area.finish_streaming();
        self.output_area.stop_spinner();
        self.chat.tool_call_active = false;
        self.chat.active_tool_call_ids.clear();
        self.chat.is_processing = false;
        self.status_bar.set_success("Ready");
        self.push_session_reminder_recap();
        self.maybe_auto_reflect(ui_tx);
    }
}
