use super::UpdateResult;
use crate::tui::adapter::input_widget::apply_input_changes_to_widget;
use crate::tui::app::{App, UiEvent};
use crate::tui::effect::session::processing::SpawnContextRefs;
use crate::tui::model::conversation::intent::ConversationIntent;
use tokio::sync::mpsc;

impl App {
    /// Handle Enter when not processing
    pub(super) fn update_enter(
        &mut self,
        ui_tx: &mpsc::Sender<UiEvent>,
        spawn_refs: &SpawnContextRefs,
    ) -> UpdateResult {
        let changes = self
            .model
            .input
            .apply(crate::tui::model::input::intent::InputIntent::Submit);
        let input = changes
            .iter()
            .find_map(|change| {
                if let crate::tui::model::input::change::InputChange::Submitted { submission } =
                    change
                {
                    Some(submission.text.clone())
                } else {
                    None
                }
            })
            .unwrap_or_default();
        apply_input_changes_to_widget(&mut self.input_area, &mut self.status_bar, &changes);
        if input.starts_with('/') {
            self.input.push_queue(input.clone());
            return UpdateResult {
                effects: Vec::new(),
                spawn_effect: None,
                pending_slash: Some(input),
            };
        }

        self.model
            .conversation
            .apply(ConversationIntent::StartChat {
                submission: input.clone(),
            });
        self.refresh_output_widget_from_model();

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
            self.append_error_notice("SDK agent client is unavailable");
            return UpdateResult::none();
        };
        self.chat.clear_tool_activity();
        self.output_area.start_spinner();
        self.output_area.set_spinner_phase("Thinking...");
        self.chat.start_processing();

        UpdateResult::spawn_processing(spawn_ctx)
    }
}
