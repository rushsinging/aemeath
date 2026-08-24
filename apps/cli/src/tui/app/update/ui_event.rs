use super::UpdateResult;
use crate::tui::app::{App, UiEvent};
use crate::tui::effect::effect::Effect;
use crate::tui::effect::session::processing::SpawnContextRefs;
use crate::tui::update::intent::AgentIntent;
use tokio::sync::mpsc;

impl App {
    /// Handle UI events from background processing
    pub(super) fn update_ui(
        &mut self,
        ev: UiEvent,
        _ui_tx: &mpsc::Sender<UiEvent>,
        _spawn_refs: &SpawnContextRefs,
    ) -> UpdateResult {
        let effects = Vec::new();
        match ev {
            UiEvent::Error(msg) => {
                self.chat.stop_processing();
                self.chat.clear_processing_handle();
                self.append_error_notice(&msg);
                return UpdateResult::one(Effect::RunHook {
                    message: msg,
                    name: "error".to_string(),
                });
            }
            UiEvent::ClipboardImage(img) => {
                self.handle_input_intent(
                    crate::tui::model::input::intent::InputIntent::InsertImage(img),
                );
            }
            UiEvent::SystemMessage(msg) => {
                self.append_system_notice(msg.clone());
                return UpdateResult::one(Effect::RunHook {
                    message: msg,
                    name: "system_message".to_string(),
                });
            }
            UiEvent::SessionSaved { id } => {
                self.append_system_notice(format!("[session saved: {id}]"));
            }
            UiEvent::WorkspaceMetadataResolved(metadata) => {
                self.apply_agent_intent(AgentIntent::Workspace(
                    crate::tui::model::workspace_provider::WorkspaceIntent::ApplyMetadata {
                        root: metadata.root,
                        revision: metadata.revision,
                        branch: metadata.branch,
                        kind: metadata.kind,
                    },
                ));
            }
            UiEvent::UpdateAvailable {
                current,
                latest,
                release_url,
            } => {
                self.append_system_notice(format!(
                    "[aemeath v{latest} is available (you have v{current}); run `aemeath update` to upgrade | {release_url}]"
                ));
            }
            UiEvent::DisplayHistoryWindowLoaded { window } => {
                let window = crate::tui::adapter::event_mapping::tui_display_history_window(window);
                self.output_view.loading_history_window = None;
                if self.model.display_history.apply_window(window) {
                    self.output_view.retained.invalidate_display_history();
                    self.mark_output_dirty();
                }
            }
            UiEvent::DisplayHistoryWindowLoadFailed { request, message } => {
                let request_key = (
                    request.session_id,
                    request.generation_revision,
                    request.member_names,
                );
                if self.output_view.loading_history_window.as_ref() == Some(&request_key) {
                    self.output_view.loading_history_window = None;
                }
                crate::tui::log_warn!("display history window loading failed: {message}");
            }
        }

        UpdateResult {
            effects,
            spawn_effect: None,
            pending_slash: None,
        }
    }
}

#[cfg(test)]
#[path = "ui_event_tests.rs"]
mod ui_event_tests;
