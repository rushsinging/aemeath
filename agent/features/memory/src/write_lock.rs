//! Corpus 写锁：进程内门闩 + 可选 OS `write.lock`（独立于 storage `dataset.lock`）。
//!
//! Reflect（含 LLM 窗口）与 Update/archive 等写路径共用。
//! 同 task 嵌套：外层持 `MemoryWritePermit`，内层走 `apply_reflection_under_permit`
//! （NEVER 对同一 gate 二次 `lock_owned`，避免异步重入死锁）。

use std::sync::Arc;

use tokio::sync::{Mutex, OwnedMutexGuard};

/// OS 级 corpus 写锁句柄（持有即互斥；Drop 释放 flock）。
pub struct OsWriteLockHandle {
    _file: std::fs::File,
}

impl OsWriteLockHandle {
    pub(crate) fn from_file(file: std::fs::File) -> Self {
        Self { _file: file }
    }
}

/// RAII 写许可。Drop 时释放进程内门闩与 OS flock。
pub struct MemoryWritePermit {
    kind: PermitKind,
}

enum PermitKind {
    Noop,
    Held {
        _guard: OwnedMutexGuard<()>,
        _os: Vec<OsWriteLockHandle>,
    },
}

pub(crate) struct WriteGate {
    mutex: Arc<Mutex<()>>,
}

impl WriteGate {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            mutex: Arc::new(Mutex::new(())),
        })
    }

    pub(crate) async fn acquire(
        self: &Arc<Self>,
        os_locks: Vec<OsWriteLockHandle>,
    ) -> MemoryWritePermit {
        let guard = self.mutex.clone().lock_owned().await;
        MemoryWritePermit {
            kind: PermitKind::Held {
                _guard: guard,
                _os: os_locks,
            },
        }
    }
}

impl MemoryWritePermit {
    pub fn noop() -> Self {
        Self {
            kind: PermitKind::Noop,
        }
    }

    /// 供 `*_under_permit` 路径做调试断言：noop 与 held 均可。
    pub fn is_held(&self) -> bool {
        matches!(self.kind, PermitKind::Held { .. }) || matches!(self.kind, PermitKind::Noop)
    }
}

#[cfg(test)]
#[path = "write_lock_port_tests.rs"]
mod port_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn permit_blocks_second_task_until_released() {
        let gate = WriteGate::new();
        let first = gate.acquire(Vec::new()).await;
        let gate2 = Arc::clone(&gate);
        let blocked = tokio::spawn(async move { gate2.acquire(Vec::new()).await });
        tokio::time::sleep(Duration::from_millis(80)).await;
        assert!(!blocked.is_finished(), "第二任务在写许可释放前 MUST 阻塞");
        drop(first);
        let second = tokio::time::timeout(Duration::from_secs(2), blocked)
            .await
            .expect("释放后应在超时内获得许可")
            .expect("join");
        drop(second);
    }
}
