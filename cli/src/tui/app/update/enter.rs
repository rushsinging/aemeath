use super::UpdateResult;
use crate::tui::app::msg::Cmd;
use crate::tui::app::processing::SpawnContextRefs;
use crate::tui::app::{App, UiEvent};
use aemeath_core::message::Message;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

impl App {
    /// Handle Enter when not processing
    pub(super) fn update_enter(
        &mut self,
        ui_tx: &mpsc::Sender<UiEvent>,
        active_cancel: &Arc<std::sync::Mutex<Option<CancellationToken>>>,
        spawn_refs: &SpawnContextRefs<'_>,
    ) -> UpdateResult {
        let input = self.input_area.get_text();
        if input.starts_with('/') {
            self.input_area.add_history(&input);
            self.input_area.clear();
            self.input_queue.push_back(input.clone());
            return UpdateResult {
                cmd: Cmd::None,
                pending_slash: Some(input),
            };
        }

        self.output_area.push_user_message(&input);
        self.input_area.add_history(&input);
        self.input_area.clear();

        let images: Vec<(String, String)> = self
            .pending_images
            .drain(..)
            .map(|img| (img.base64, img.media_type))
            .collect();
        if images.is_empty() {
            self.messages.push(Message::user(&input));
        } else {
            self.messages
                .push(Message::user_with_images(&input, images));
        }

        let spawn_ctx = self.build_spawn_context(ui_tx, active_cancel, spawn_refs);
        spawn_refs.interrupted.store(false, Ordering::Relaxed);
        self.active_tool_call_ids.clear();
        self.tool_call_active = false;
        self.output_area.start_spinner();
        self.output_area.set_spinner_phase("Thinking...");
        self.is_processing = true;

        UpdateResult {
            cmd: Cmd::SpawnProcessing(spawn_ctx),
            pending_slash: None,
        }
    }
}
