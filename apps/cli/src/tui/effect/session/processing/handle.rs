use crate::tui::app::event::{UiEvent, UiTurnContext};
use std::sync::Arc;
use tokio::sync::mpsc;

use super::input_port::TuiInputEventPort;

pub(crate) struct SpawnContextRefs {
    pub agent_client: Option<Arc<dyn sdk::AgentClient>>,
}

pub(crate) struct SpawnContext {
    pub tx: mpsc::Sender<UiEvent>,
    pub input_event_port: TuiInputEventPort,
    pub agent_client: Arc<dyn sdk::AgentClient>,
    pub fallback_context: UiTurnContext,
}

pub(crate) struct ProcessingHandle {
    pub(super) join: tokio::task::JoinHandle<()>,
    pub(super) agent_client: Arc<dyn sdk::AgentClient>,
    pub(super) active_run_id: Arc<std::sync::Mutex<Option<sdk::RunId>>>,
    pub(super) pending_cancel: Arc<std::sync::atomic::AtomicBool>,
}

impl std::fmt::Debug for ProcessingHandle {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProcessingHandle")
            .finish_non_exhaustive()
    }
}

impl ProcessingHandle {
    pub(crate) fn cancel_current_run(&self) -> sdk::CancelRunOutcome {
        let run_id = self
            .active_run_id
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        if let Some(run_id) = run_id.as_ref() {
            self.agent_client.cancel_run(run_id)
        } else {
            self.pending_cancel
                .store(true, std::sync::atomic::Ordering::Release);
            sdk::CancelRunOutcome::Accepted
        }
    }

    pub(crate) fn abort(&self) {
        self.join.abort();
    }

    pub(crate) fn is_finished(&self) -> bool {
        self.join.is_finished()
    }
}

/// #567 S5：退出时等待 spawn task 完成（含 auto-save），超时则放弃。
pub(crate) async fn shutdown_and_save(handle: Option<ProcessingHandle>) {
    if let Some(handle) = handle {
        // 先 abort 如果已卡死——但给 loop 一点时间自然退出 + auto-save。
        // JoinHandle.await 在 tokio runtime 中等待 task 完成。
        let timeout = tokio::time::timeout(std::time::Duration::from_secs(5), handle.join).await;
        if timeout.is_err() {
            crate::tui::log_warn!("auto-save timed out, forcing abort");
        }
    }
}
