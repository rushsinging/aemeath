use super::UpdateResult;
use crate::tui::core::input_adapter::apply_input_changes_to_widget;
use crate::tui::core::{App, UiEvent};
use crate::tui::session::processing::SpawnContextRefs;
use tokio::sync::mpsc;

impl App {
    /// Handle Enter when not processing
    pub(super) fn update_enter(
        &mut self,
        ui_tx: &mpsc::Sender<UiEvent>,
        spawn_refs: &SpawnContextRefs,
    ) -> UpdateResult {
        let input = self.input_area.get_text();
        let changes = self
            .model
            .input
            .apply(crate::tui::model::input::intent::InputIntent::Submit);
        apply_input_changes_to_widget(&mut self.input_area, &mut self.status_bar, &changes);
        if input.starts_with('/') {
            self.input.push_queue(input.clone());
            return UpdateResult {
                effects: Vec::new(),
                spawn_effect: None,
                pending_slash: Some(input),
            };
        }

        self.output_area.push_user_message(&input);

        let images: Vec<sdk::ToolResultImage> = self
            .chat
            .drain_pending_images()
            .into_iter()
            .map(Into::into)
            .collect();
        if images.is_empty() {
            self.chat.messages.push(sdk::ChatMessage::user_text(&input));
        } else {
            self.chat
                .messages
                .push(sdk::ChatMessage::user_with_images(&input, images));
        }

        let Some(spawn_ctx) = self.build_spawn_context(ui_tx, spawn_refs) else {
            self.output_area
                .push_error("SDK agent client is unavailable");
            return UpdateResult::none();
        };
        self.chat.clear_tool_activity();
        self.output_area.start_spinner();
        self.output_area.set_spinner_phase("Thinking...");
        self.chat.start_processing();

        UpdateResult::spawn_processing(spawn_ctx)
    }
}
