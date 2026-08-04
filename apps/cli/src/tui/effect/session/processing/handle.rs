use crate::tui::adapter::tui_runtime_event::{TuiRunContext, TuiRuntimeEvent};
use crate::tui::app::event::UiEvent;
use std::sync::Arc;
use tokio::sync::mpsc;

use super::input_port::TuiInputEventPort;

pub(crate) struct SpawnContextRefs {
    pub agent_client: Option<Arc<dyn sdk::AgentClient>>,
}

pub(crate) struct SpawnContext {
    pub runtime_tx: mpsc::Sender<TuiRuntimeEvent>,
    pub local_tx: mpsc::Sender<UiEvent>,
    pub input_event_port: TuiInputEventPort,
    pub agent_client: Arc<dyn sdk::AgentClient>,
    pub fallback_context: TuiRunContext,
}

pub(crate) struct ProcessingHandle {
    pub(super) join: tokio::task::JoinHandle<()>,
    pub(super) agent_client: Arc<dyn sdk::AgentClient>,
}

impl std::fmt::Debug for ProcessingHandle {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProcessingHandle")
            .finish_non_exhaustive()
    }
}

impl ProcessingHandle {
    pub(crate) fn cancel_current_run(&self) -> sdk::CancelCurrentRunOutcome {
        let deadline = sdk::ControlDeadline::from_unix_millis(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_millis() as u64 + 10_000)
                .unwrap_or(0),
        );
        crate::tui::log_debug!(
            "processing handle forwarding cancel_current_run: join_finished={} deadline_unix_ms={}",
            self.join.is_finished(),
            deadline.unix_millis()
        );
        let outcome = self.agent_client.cancel_current_run(deadline);
        crate::tui::log_debug!(
            "processing handle received cancel_current_run outcome: outcome={:?}",
            outcome
        );
        outcome
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
