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
}

impl BackgroundTaskRuntime {
    pub(crate) fn new() -> Self {
        let (notifier, waiter) = wakeup::wakeup_channel();
        Self {
            supervisor: Arc::new(BackgroundTaskSupervisor::new()),
            notifier,
            waiter: Mutex::new(Some(waiter)),
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

impl Default for BackgroundTaskRuntime {
    fn default() -> Self {
        Self::new()
    }
}
