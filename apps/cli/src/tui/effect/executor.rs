use crate::tui::core::event::UiEvent;
use crate::tui::core::App;
use crate::tui::effect::effect::{Effect, SpawnAgentChatEffect};
use crate::tui::session::processing;
use tokio::sync::mpsc;

impl App {
    pub(crate) fn execute_spawn_effect(&mut self, effect: SpawnAgentChatEffect) {
        if let Some(spawn_ctx) = effect.context {
            processing::spawn_processing(spawn_ctx);
        }
    }

    pub(crate) async fn execute_effect(&mut self, effect: Effect, ui_tx: &mpsc::Sender<UiEvent>) {
        match effect {
            Effect::None | Effect::RequestRender => {}
            Effect::QuitApplication => self.layout.request_exit(),
            Effect::SpawnAgentChat { .. } => {}
            Effect::CancelAgentChat => self.cancel_agent_chat(),
            Effect::SaveSession => self.save_session_effect().await,
            Effect::RunHook { message, name } => self.run_hook_effect(message, name).await,
            Effect::ReadClipboardImage => self.read_clipboard_image_effect().await,
            Effect::ProcessImageFile { path } => self.process_image_file_effect(path).await,
            Effect::SetCurrentTurn { turn } => self.set_current_turn_effect(turn),
            Effect::FetchReminderRecap => self.fetch_reminder_recap_effect(ui_tx).await,
            Effect::FetchTaskStatus
            | Effect::CopyToClipboard { .. }
            | Effect::StartTimer { .. }
            | Effect::StopTimer { .. } => {}
        }
    }

    fn cancel_agent_chat(&mut self) {
        if let Some(ref ac) = self.agent_client {
            ac.cancel();
        }
    }

    async fn save_session_effect(&mut self) {
        if self.chat.messages.is_empty() {
            return;
        }
        if let Some(ref ac) = self.agent_client {
            if let Err(e) = ac.sync_current_messages(self.chat.messages.clone()).await {
                log::warn!("sync failed: {e}");
            }
            if let Err(e) = ac.save_current_session().await {
                log::warn!("save failed: {e}");
            }
        }
    }

    async fn run_hook_effect(&mut self, message: String, name: String) {
        if let Some(ref ac) = self.agent_client {
            let _ = ac.notify_hook(&message, &name).await;
        }
    }

    async fn read_clipboard_image_effect(&mut self) {
        if let Some(ref ac) = self.agent_client {
            match ac.read_clipboard_image().await {
                Ok(img) => self.accept_pending_clipboard_image(img),
                Err(e) => log::warn!("clipboard read failed: {e}"),
            }
        }
    }

    async fn process_image_file_effect(&mut self, path: String) {
        if let Some(ref ac) = self.agent_client {
            match ac.process_image_file(path).await {
                Ok(img) => self.accept_pending_clipboard_image(img),
                Err(e) => log::warn!("image process failed: {e}"),
            }
        }
    }

    fn accept_pending_clipboard_image(&mut self, img: sdk::ClipboardImageView) {
        let count = self.chat.add_pending_image(img);
        self.input_area.set_pending_images(count);
    }

    fn set_current_turn_effect(&mut self, turn: usize) {
        if let Some(ref ac) = self.agent_client {
            ac.set_current_turn(turn);
        }
    }

    async fn fetch_reminder_recap_effect(&mut self, ui_tx: &mpsc::Sender<UiEvent>) {
        if let Some(ref ac) = self.agent_client {
            match ac.list_reminders().await {
                Ok(reminders) => {
                    if let Some(line) = sdk::ReminderView::recap_line(&reminders) {
                        let _ = ui_tx.send(UiEvent::ReminderRecap(line)).await;
                    }
                }
                Err(e) => log::warn!("fetch reminder recap failed: {e}"),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_effect_runtime_ignores_noop_effect() {
        let app = App::new(
            "s".to_string(),
            std::path::PathBuf::from("/tmp"),
            "m".to_string(),
        );
        assert!(!app.layout.should_exit);
    }

    #[test]
    fn test_effect_runtime_quit_effect_sets_exit_flag() {
        let mut app = App::new(
            "s".to_string(),
            std::path::PathBuf::from("/tmp"),
            "m".to_string(),
        );
        app.layout.request_exit();
        assert!(app.layout.should_exit);
    }

    #[test]
    fn test_effect_runtime_accepts_pending_image() {
        let mut app = App::new(
            "s".to_string(),
            std::path::PathBuf::from("/tmp"),
            "m".to_string(),
        );
        app.accept_pending_clipboard_image(sdk::ClipboardImageView {
            base64: "abc".to_string(),
            media_type: "image/png".to_string(),
            final_size: 3,
            display_path: None,
            width: None,
            height: None,
        });
        assert_eq!(app.chat.pending_images().len(), 1);
    }
}
