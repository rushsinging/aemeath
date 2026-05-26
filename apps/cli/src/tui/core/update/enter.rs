use super::UpdateResult;
use crate::tui::core::msg::Cmd;
use crate::tui::core::{App, UiEvent};
use crate::tui::session::processing::SpawnContextRefs;
use ::runtime::api::core::message::Message;
use tokio::sync::mpsc;

impl App {
    /// Handle Enter when not processing
    pub(super) fn update_enter(
        &mut self,
        ui_tx: &mpsc::Sender<UiEvent>,
        spawn_refs: &SpawnContextRefs,
    ) -> UpdateResult {
        let input = self.input_area.get_text();
        if input.starts_with('/') {
            self.input_area.add_history(&input);
            self.input_area.clear();
            self.input.input_queue.push_back(input.clone());
            return UpdateResult {
                cmd: Cmd::None,
                pending_slash: Some(input),
            };
        }

        self.output_area.push_user_message(&input);
        self.input_area.add_history(&input);
        self.input_area.clear();

        let images: Vec<(String, String)> = self
            .chat
            .pending_images
            .drain(..)
            .map(|img| (img.base64, img.media_type))
            .collect();
        if images.is_empty() {
            self.chat.messages.push(Message::user(&input));
        } else {
            self.chat
                .messages
                .push(Message::user_with_images(&input, images));
        }

        let Some(spawn_ctx) = self.build_spawn_context(ui_tx, spawn_refs) else {
            self.output_area
                .push_error("SDK agent client is unavailable");
            return UpdateResult {
                cmd: Cmd::None,
                pending_slash: None,
            };
        };
        self.chat.active_tool_call_ids.clear();
        self.chat.tool_call_active = false;
        self.output_area.start_spinner();
        self.output_area.set_spinner_phase("Thinking...");
        self.chat.is_processing = true;

        UpdateResult {
            cmd: Cmd::SpawnProcessing(spawn_ctx),
            pending_slash: None,
        }
    }
}
