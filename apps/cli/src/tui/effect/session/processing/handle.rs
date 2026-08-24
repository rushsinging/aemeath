use crate::tui::adapter::tui_runtime_event::{TuiRunContext, TuiRuntimeEvent};
use std::sync::Arc;
use tokio::sync::mpsc;

use super::input_port::TuiInputEventPort;

pub(crate) struct SpawnContextRefs {
    pub agent_client: Option<Arc<dyn sdk::AgentClient>>,
}

pub(crate) struct SpawnContext {
    pub runtime_tx: mpsc::Sender<TuiRuntimeEvent>,
    pub input_event_port: TuiInputEventPort,
    pub agent_client: Arc<dyn sdk::AgentClient>,
    pub fallback_context: TuiRunContext,
}

pub(crate) struct ProcessingHandle {
    pub(super) join: tokio::task::JoinHandle<()>,
}

impl std::fmt::Debug for ProcessingHandle {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProcessingHandle")
            .finish_non_exhaustive()
    }
}

impl ProcessingHandle {
    pub(crate) fn abort(&self) {
        self.join.abort();
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
