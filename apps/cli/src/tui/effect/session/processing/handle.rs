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

#[derive(Debug)]
pub(crate) struct ProcessingHandle {
    pub(super) join: tokio::task::JoinHandle<()>,
    /// #639：runtime chat 的 cancel 句柄。因 `chat()` 在 spawn task **内部** await，
    /// 句柄要等 chat() 返回后才拿得到，故用共享 slot 由 task 回填、TUI 侧读取触发。
    pub(super) cancel: Arc<std::sync::Mutex<Option<sdk::CancelHandle>>>,
}

impl ProcessingHandle {
    /// #639：即时取消当前 chat（触发 runtime 的 CancellationToken，进程内 out-of-band，
    /// 不走事件流）。abort 只中断 TUI 消费流不停 runtime loop，故 cancel NEVER 用 abort。
    pub(crate) fn cancel(&self) {
        if let Ok(guard) = self.cancel.lock() {
            if let Some(handle) = guard.as_ref() {
                handle.cancel();
            }
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
