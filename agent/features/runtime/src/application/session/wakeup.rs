//! 后台任务唤醒信箱（#252 PR2）：无 active Run 时的 Wakeup 信号通道。
//!
//! 任务终态若此刻没有 active Main Run，完成通知无法经 reminder 注入
//! （reminder 管线是 Run 级的）——改由本信箱把「有后台任务完成」信号
//! 送达 session driver 的 idle 等待点，驱动一次 Wakeup Run
//! （`RunIntent::BackgroundTaskWakeup`）。信号是纯触发语义：完成事实
//! 由 `background_task` reminder 在 Wakeup Run 内注入（D11），信箱不
//! 携带任务数据。

use tokio::sync::mpsc;

/// 唤醒发送端：监督器通知路由持有。
pub(crate) struct WakeupNotifier {
    sender: mpsc::UnboundedSender<()>,
}

/// 唤醒等待端：session driver idle 等待点持有。
pub(crate) struct WakeupWaiter {
    receiver: mpsc::UnboundedReceiver<()>,
}

/// 建立唤醒通道（session 级一份）。
pub(crate) fn wakeup_channel() -> (WakeupNotifier, WakeupWaiter) {
    let (sender, receiver) = mpsc::unbounded_channel();
    (WakeupNotifier { sender }, WakeupWaiter { receiver })
}

impl WakeupNotifier {
    /// 发送唤醒信号；发送端存活时不可失败（unbounded）。
    pub(crate) fn wakeup(&self) -> Result<(), mpsc::error::SendError<()>> {
        self.sender.send(())
    }
}

impl WakeupWaiter {
    /// 阻塞等待唤醒信号；发送端全部释放（session 退出）返回 None。
    pub(crate) async fn wait(&mut self) -> Option<()> {
        self.receiver.recv().await
    }

    /// 非消耗性探测：有待处理信号时取一个（合流语义，只用于测试与诊断）。
    #[cfg(test)]
    pub(crate) fn try_wait(&mut self) -> Option<()> {
        self.receiver.try_recv().ok()
    }
}

#[cfg(test)]
#[path = "wakeup_tests.rs"]
mod tests;
