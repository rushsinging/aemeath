use crate::tui::app::processing::{SpawnContext, SpawnContextRefs};
use crate::tui::app::{App, UiEvent};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

impl App {
    /// Build an owned SpawnContext from borrowed refs
    pub(super) fn build_spawn_context(
        &mut self,
        ui_tx: &mpsc::Sender<UiEvent>,
        _active_cancel: &Arc<std::sync::Mutex<Option<CancellationToken>>>,
        spawn_refs: &SpawnContextRefs<'_>,
    ) -> SpawnContext {
        let cancel = CancellationToken::new();
        // Note: active_cancel is set by the caller after getting the Cmd back
        SpawnContext {
            tx: ui_tx.clone(),
            queue_request_tx: ui_tx.clone(),
            client: spawn_refs.client.clone(),
            registry: spawn_refs.registry.clone(),
            system_blocks: spawn_refs.system_blocks.clone(),
            system_prompt_text: spawn_refs.system_prompt_text.to_string(),
            user_context: spawn_refs.user_context.to_string(),
            messages: self.chat.messages.clone(),
            context_size: spawn_refs.context_size,
            cwd: self.session.cwd.clone(),
            workspace_context: self.workspace_context.clone(),
            session_id: self.session.session_id.clone(),
            read_files: spawn_refs.read_files.clone(),
            session_reminders: spawn_refs.session_reminders.clone(),
            agent_runner: spawn_refs.agent_runner.clone(),
            allow_all: spawn_refs.allow_all,
            interrupted: spawn_refs.interrupted.clone(),
            cancel,
            task_store: spawn_refs.task_store.clone(),
            max_tool_concurrency: spawn_refs.max_tool_concurrency,
            max_agent_concurrency: spawn_refs.max_agent_concurrency,
            agent_semaphore: spawn_refs.agent_semaphore.clone(),
            hook_runner: spawn_refs.hook_runner.clone(),
            memory_config: spawn_refs.memory_config.clone(),
            json_logger: spawn_refs.json_logger.clone(),
        }
    }
}
