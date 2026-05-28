use crate::tui::app::{App, UiEvent};
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
        Some(SpawnContext {
            tx: ui_tx.clone(),
            queue_request_tx: ui_tx.clone(),
            agent_client,
            messages: self.chat.messages.clone(),
        })
    }
}
