//! 异步落盘 worker：诊断日志行经有界 channel 转发到专用线程写盘。
//!
//! 根因背景：`UnifiedLogger::log()` 曾在调用线程（含 TUI 主线程）同步执行
//! `stat + write + flush`；磁盘繁忙时单条日志可阻塞数秒，导致 TUI 回车后
//! 单帧 prepare 卡顿（`tui_slow_frame prepare_ms=3724`）。此模块把全部文件
//! IO 收敛到唯一 worker 线程：调用方只做 `try_send`，channel 满时丢弃并
//! 计数，绝不反压调用线程。

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TrySendError};
use std::sync::Arc;
use std::thread::JoinHandle;

use super::lifecycle::{EmergencyWriter, FileSinkLifecycle};
use crate::domain::DiagnosticSinkId;

/// 发送给 worker 线程的命令。
pub(super) enum SinkCommand {
    /// 写一行到指定 sink。
    WriteLine {
        sink: DiagnosticSinkId,
        line: String,
    },
    /// 排空屏障：worker 处理完此命令之前的所有 WriteLine 并 flush 后 ack。
    FlushBarrier { done: SyncSender<()> },
}

/// 异步 sink 的调用方句柄：线程安全，`try_send` 永不阻塞。
pub(super) struct AsyncSinkHandle {
    sender: SyncSender<SinkCommand>,
    dropped_lines: Arc<AtomicU64>,
}

impl AsyncSinkHandle {
    /// 入队一行日志。channel 满或 worker 已停止时丢弃该行并累加丢弃计数。
    pub(super) fn enqueue_line(&self, sink: DiagnosticSinkId, line: String) {
        if self
            .sender
            .try_send(SinkCommand::WriteLine { sink, line })
            .is_err()
        {
            self.dropped_lines.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// 当前累计丢弃行数（含 channel 满与 worker 停止两类丢弃）。
    /// 生产路径经 FlushBarrier 的 emergency 报告观测；此读取器供测试断言。
    #[cfg_attr(not(test), allow(dead_code))]
    pub(super) fn dropped_lines(&self) -> u64 {
        self.dropped_lines.load(Ordering::Relaxed)
    }

    /// 同步排空屏障：阻塞调用线程直到 worker 写完此前入队的全部行并 flush。
    /// worker 已停止时立即返回（不阻塞）。
    pub(super) fn flush_barrier(&self) {
        let (done_sender, done_receiver) = std::sync::mpsc::sync_channel::<()>(1);
        // channel 可能被日志行占满：忙等入队不丢屏障语义（屏障必须排在
        // 此前所有行之后），worker 持续消费最终腾出空位。
        loop {
            let barrier = SinkCommand::FlushBarrier {
                done: done_sender.clone(),
            };
            match self.sender.try_send(barrier) {
                Ok(()) => break,
                Err(TrySendError::Full(_)) => std::thread::yield_now(),
                Err(TrySendError::Disconnected(_)) => return,
            }
        }
        let _ = done_receiver.recv();
    }
}

/// 专用落盘线程：独占持有全部 `FileSinkLifecycle`，按入队顺序写盘。
pub(super) struct AsyncSinkWorker {
    handle: AsyncSinkHandle,
    #[cfg_attr(not(test), allow(dead_code))]
    worker: JoinHandle<HashMap<DiagnosticSinkId, FileSinkLifecycle>>,
}

impl AsyncSinkWorker {
    /// 启动 worker 线程并返回调用方句柄。`capacity` 为 channel 容量（至少 1）。
    pub(super) fn spawn(
        lifecycles: HashMap<DiagnosticSinkId, FileSinkLifecycle>,
        emergency: Arc<dyn EmergencyWriter>,
        capacity: usize,
    ) -> Self {
        let (sender, receiver) = std::sync::mpsc::sync_channel(capacity.max(1));
        let dropped_lines = Arc::new(AtomicU64::new(0));
        let worker_dropped = Arc::clone(&dropped_lines);
        let worker = std::thread::Builder::new()
            .name("aemeath-async-log-sink".to_string())
            .spawn(move || worker_loop(receiver, lifecycles, emergency, worker_dropped))
            .expect("spawn async log sink thread");
        Self {
            handle: AsyncSinkHandle {
                sender,
                dropped_lines,
            },
            worker,
        }
    }

    /// 调用方句柄。
    pub(super) fn handle(&self) -> &AsyncSinkHandle {
        &self.handle
    }

    /// 等待 worker 线程退出并取回 lifecycles。生产进程中 logger 被 leak、
    /// 生命周期与进程一致，不做 join；本方法用于测试的确定性收尾。
    #[cfg_attr(not(test), allow(dead_code))]
    pub(super) fn join(self) -> HashMap<DiagnosticSinkId, FileSinkLifecycle> {
        drop(self.handle);
        self.worker.join().unwrap_or_default()
    }
}

fn worker_loop(
    receiver: Receiver<SinkCommand>,
    mut lifecycles: HashMap<DiagnosticSinkId, FileSinkLifecycle>,
    emergency: Arc<dyn EmergencyWriter>,
    dropped_lines: Arc<AtomicU64>,
) -> HashMap<DiagnosticSinkId, FileSinkLifecycle> {
    while let Ok(command) = receiver.recv() {
        match command {
            SinkCommand::WriteLine { sink, line } => {
                if let Some(lifecycle) = lifecycles.get_mut(&sink) {
                    lifecycle.write_line(&line);
                }
            }
            SinkCommand::FlushBarrier { done } => {
                let dropped_total = dropped_lines.swap(0, Ordering::Relaxed);
                for lifecycle in lifecycles.values_mut() {
                    lifecycle.flush();
                }
                if dropped_total > 0 {
                    emergency.write(&format!(
                        "async log sink dropped={dropped_total} diagnostic lines"
                    ));
                }
                let _ = done.send(());
            }
        }
    }
    lifecycles
}

#[cfg(test)]
#[path = "async_sink_tests.rs"]
mod tests;
