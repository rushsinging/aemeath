//! 后台任务 session 运行时（#252 PR2）：监督器 + 唤醒信箱的 session 级装配。

use std::sync::{Arc, Mutex};

use super::supervisor::BackgroundTaskSupervisor;
use crate::application::session::wakeup::{self, WakeupNotifier, WakeupWaiter};

/// Session 级后台任务运行时：挂在 `SessionRuntime`（跨 Run 共享）。
///
/// - `supervisor`：任务事实账本（登记 / 状态推进 / 未通知终态 take）。
/// - 唤醒信箱：任务终态且无 active Run 时发信号，session driver idle
///   等待点 select（Wakeup Run，D13）。
pub(crate) struct BackgroundTaskRuntime {
    supervisor: Arc<BackgroundTaskSupervisor>,
    notifier: WakeupNotifier,
    waiter: Mutex<Option<WakeupWaiter>>,
    active_run:
        std::sync::OnceLock<Arc<crate::application::run::active_registry::ActiveRunRegistry>>,
}

/// 终态通知路由决策（纯判定，#252 PR2 §4.2/§4.4）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BackgroundNotifyRoute {
    /// 有 active Main Run：`background_task` reminder 事件注入该 Run。
    Reminder(sdk::RunId),
    /// 无 active Run：wakeup 信号驱动 Wakeup Run。
    WakeupSignal,
}

impl BackgroundTaskRuntime {
    pub(crate) fn new() -> Self {
        let (notifier, waiter) = wakeup::wakeup_channel();
        Self {
            supervisor: Arc::new(BackgroundTaskSupervisor::new()),
            notifier,
            waiter: Mutex::new(Some(waiter)),
            active_run: std::sync::OnceLock::new(),
        }
    }

    /// 绑定 active run registry（SessionRuntime::new 收尾时调用，一次绑定）。
    pub(crate) fn bind_active_run(
        &self,
        registry: Arc<crate::application::run::active_registry::ActiveRunRegistry>,
    ) {
        let _ = self.active_run.set(registry);
    }

    #[cfg(test)]
    pub(crate) fn for_test(
        active_run: Arc<crate::application::run::active_registry::ActiveRunRegistry>,
    ) -> Self {
        let runtime = Self::new();
        runtime.bind_active_run(active_run);
        runtime
    }

    /// 通知路由判定：有 active Main Run → reminder 事件；无 → wakeup 信号。
    pub(crate) fn notify_route(&self) -> BackgroundNotifyRoute {
        let Some(registry) = self.active_run.get() else {
            return BackgroundNotifyRoute::WakeupSignal;
        };
        match registry.current_main_run_id() {
            Some(run_id) => BackgroundNotifyRoute::Reminder(run_id),
            None => BackgroundNotifyRoute::WakeupSignal,
        }
    }

    /// 任务终态通知（spawn body 调用）：推进监督器终态并按路由送达。
    ///
    /// 重复终态幂等（finish 返回 changed=false 时不再通知）。
    pub(crate) fn notify_terminal(
        &self,
        context: &crate::application::context::coordination::ContextCoordinator,
        task_id: &share::ids::BackgroundTaskId,
        kind: crate::domain::background_task::BackgroundTaskTerminalKind,
        terminal_output: Option<String>,
    ) {
        let finished = self
            .supervisor
            .finish(task_id, kind, terminal_output)
            .unwrap_or(false);
        if !finished {
            return;
        }
        match self.notify_route() {
            BackgroundNotifyRoute::Reminder(run_id) => {
                context.reminder_handle_event(
                    &run_id,
                    &context::ReminderEventSource::background_task(),
                );
            }
            BackgroundNotifyRoute::WakeupSignal => {
                if let Err(error) = self.notifier.wakeup() {
                    log::warn!(
                        target: crate::LOG_TARGET,
                        "background wakeup signal lost (session exiting): {error:?}"
                    );
                }
            }
        }
    }

    pub(crate) fn supervisor(&self) -> Arc<BackgroundTaskSupervisor> {
        self.supervisor.clone()
    }

    pub(crate) fn notifier(&self) -> &WakeupNotifier {
        &self.notifier
    }

    /// 取走等待端（session driver 首次接线；一个 session 一个等待端）。
    pub(crate) fn take_wakeup_waiter(&self) -> Option<WakeupWaiter> {
        self.waiter.lock().expect("后台任务唤醒信箱锁中毒").take()
    }
}

#[cfg(test)]
#[path = "session_runtime_tests.rs"]
mod tests;
