use crate::tui::app::{App, UiEvent, UiTurnContext};
use crate::tui::effect::session::processing::{SpawnContext, SpawnContextRefs};
use tokio::sync::mpsc;

impl App {
    /// Build an owned SpawnContext from borrowed refs
    pub(crate) fn build_spawn_context(
        &mut self,
        ui_tx: &mpsc::Sender<UiEvent>,
        spawn_refs: &SpawnContextRefs,
    ) -> Option<SpawnContext> {
        let agent_client = spawn_refs.agent_client.as_ref()?.clone();
        let input_event_port = self.chat.start_input_event_buffer();
        Some(SpawnContext {
            tx: ui_tx.clone(),
            input_event_port,
            agent_client,
            fallback_context: self.fallback_runtime_context(),
        })
    }

    fn fallback_runtime_context(&self) -> UiTurnContext {
        self.model
            .conversation
            .chats
            .iter()
            .rev()
            .find_map(|chat| {
                chat.turns.last().map(|turn| UiTurnContext {
                    chat_id: chat.id.clone(),
                    turn_id: turn.id.clone(),
                })
            })
            .unwrap_or_else(|| UiTurnContext {
                chat_id: crate::tui::model::conversation::ids::ChatId::new_v7(),
                turn_id: crate::tui::model::conversation::ids::ChatTurnId::new_v7(),
            })
    }
}
