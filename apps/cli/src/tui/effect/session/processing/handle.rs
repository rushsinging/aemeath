use crate::tui::adapter::tui_runtime_event::{TuiRuntimeEvent, TuiTurnContext};
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
    pub fallback_context: TuiTurnContext,
}

pub(crate) enum RunCancelState {
    Idle,
    AwaitingStart { cancel_requested: bool },
    Active(sdk::RunId),
}

pub(crate) struct ProcessingHandle {
    pub(super) join: tokio::task::JoinHandle<()>,
    pub(super) agent_client: Arc<dyn sdk::AgentClient>,
    pub(super) run_cancel_state: Arc<std::sync::Mutex<RunCancelState>>,
}

impl std::fmt::Debug for ProcessingHandle {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProcessingHandle")
            .finish_non_exhaustive()
    }
}

impl ProcessingHandle {
    pub(crate) fn expect_run_start(&self) {
        *self
            .run_cancel_state
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = RunCancelState::AwaitingStart {
            cancel_requested: false,
        };
    }

    pub(crate) fn cancel_current_run(&self) -> sdk::CancelRunOutcome {
        let run_id = {
            let mut state = self
                .run_cancel_state
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            match &mut *state {
                RunCancelState::Active(run_id) => Some(run_id.clone()),
                RunCancelState::AwaitingStart { cancel_requested } => {
                    *cancel_requested = true;
                    None
                }
                RunCancelState::Idle => return sdk::CancelRunOutcome::NotFound,
            }
        };
        run_id
            .as_ref()
            .map(|run_id| self.agent_client.cancel_run(run_id))
            .unwrap_or(sdk::CancelRunOutcome::Accepted)
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
