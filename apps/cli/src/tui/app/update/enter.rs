use super::UpdateResult;
use crate::tui::app::{App, UiEvent};
use crate::tui::effect::session::processing::SpawnContextRefs;
use crate::tui::model::conversation::intent::ConversationIntent;
use crate::tui::model::input::change::submitted_submission_from_changes;
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
        let Some(submission) = submitted_submission_from_changes(&changes) else {
            return UpdateResult::none();
        };
        let input = submission.text;
        if input.is_empty() && submission.images.is_empty() {
            return UpdateResult::none();
        }
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
        self.mark_output_dirty();

        let images: Vec<sdk::ToolResultImage> = submission
            .images
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
        self.spinner_phase(crate::tui::model::runtime::spinner::SpinnerPhase::Thinking);
        self.chat.start_processing();

        UpdateResult::spawn_processing(spawn_ctx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::model::input::intent::InputIntent;
    use std::path::PathBuf;

    fn test_app() -> App {
        App::new(
            "test-session".to_string(),
            PathBuf::from("/tmp"),
            "test-model".to_string(),
        )
    }

    #[test]
    fn test_update_enter_empty_submission_is_noop() {
        let mut app = test_app();
        let (ui_tx, _ui_rx) = mpsc::channel(1);
        let spawn_refs = SpawnContextRefs { agent_client: None };

        let result = app.update_enter(&ui_tx, &spawn_refs);

        assert!(result.effects.is_empty());
        assert!(result.spawn_effect.is_none());
        assert!(result.pending_slash.is_none());
        assert_eq!(app.chat.messages.len(), 0);
    }

    #[test]
    fn test_update_enter_slash_submission_returns_pending_slash() {
        let mut app = test_app();
        app.model
            .input
            .apply(InputIntent::InsertText("/help".to_string()));
        let (ui_tx, _ui_rx) = mpsc::channel(1);
        let spawn_refs = SpawnContextRefs { agent_client: None };

        let result = app.update_enter(&ui_tx, &spawn_refs);

        assert_eq!(result.pending_slash.as_deref(), Some("/help"));
        assert!(result.effects.is_empty());
        assert!(result.spawn_effect.is_none());
    }

    #[test]
    fn test_update_enter_renders_copied_text_user_message_as_original() {
        let mut app = test_app();
        app.model
            .input
            .apply(InputIntent::InsertPastedText("a\nb\nc\nd".to_string()));
        let (ui_tx, _ui_rx) = mpsc::channel(1);
        let spawn_refs = SpawnContextRefs { agent_client: None };

        let result = app.update_enter(&ui_tx, &spawn_refs);

        assert!(result.spawn_effect.is_none());
        let has_original_user_message = app.model.conversation.blocks.iter().any(|block| {
            matches!(
                block,
                crate::tui::model::conversation::block::ConversationBlock::UserMessage { text, .. }
                    if text == "a\nb\nc\nd"
            )
        });
        assert!(
            has_original_user_message,
            "首轮 user message 应渲染复制原文，而不是 [Copied N lines] 占位符"
        );
        assert!(app
            .chat
            .messages
            .iter()
            .any(|message| { message.role == "user" && message.text_content() == "a\nb\nc\nd" }));
    }
}
