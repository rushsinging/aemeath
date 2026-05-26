use crate::tui::core::{App, UiEvent};
use crate::tui::session::processing::{SpawnContext, SpawnContextRefs};
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
