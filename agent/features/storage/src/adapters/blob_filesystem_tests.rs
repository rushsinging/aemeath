use std::ffi::OsString;
use std::str::FromStr;
use std::sync::{Mutex, MutexGuard, OnceLock};

use super::FileSystemBlobAdapter;
use crate::domain::{Durability, SafePathSegment, StorageKey, StorageNamespace, WriteOptions};
use crate::test_log;
use crate::AtomicBlobPort;

fn fault_env_lock() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|error| error.into_inner())
}

struct FaultEnvGuard {
    previous: Option<OsString>,
    _lock: MutexGuard<'static, ()>,
}

impl FaultEnvGuard {
    fn cleanup() -> Self {
        let lock = fault_env_lock();
        let previous = std::env::var_os("AEMEATH_STORAGE_FAULT_POINT");
        std::env::set_var("AEMEATH_STORAGE_FAULT_POINT", "cleanup");
        Self {
            previous,
            _lock: lock,
        }
    }
}

impl Drop for FaultEnvGuard {
    fn drop(&mut self) {
        match self.previous.take() {
            Some(value) => std::env::set_var("AEMEATH_STORAGE_FAULT_POINT", value),
            None => std::env::remove_var("AEMEATH_STORAGE_FAULT_POINT"),
        }
    }
}

fn root() -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "aemeath-storage-blob-recovery-log-{}",
        uuid::Uuid::new_v4()
    ))
}

fn key() -> StorageKey {
    StorageKey::new(
        StorageNamespace::Session,
        vec![SafePathSegment::from_str("log-test").unwrap()],
    )
    .unwrap()
}

/// 当 write_atomic 越过逻辑提交点（crossed_commit = true）后在 cleanup 阶段
/// 故障时，返回 committed JournalCleanupPending 收据——必须同时 emit 一条
/// Warn 级 recovery_pending 日志，且不泄露 key / 路径。
#[allow(
    clippy::await_holding_lock,
    reason = "故障环境变量是进程全局状态，测试必须在整个异步提交期间独占它"
)]
#[tokio::test(flavor = "current_thread")]
async fn cleanup_fault_emits_recovery_pending_warn() {
    let root = root();
    let adapter = FileSystemBlobAdapter::new(&root).expect("adapter init");

    // 第一次写入：成功建立 primary。
    adapter
        .write_atomic(
            &key(),
            b"v1",
            WriteOptions::new(Durability::ProcessCrashSafe),
        )
        .await
        .expect("first write must succeed");

    // 在 cleanup（post-commit）注入故障：crossed_commit 已为 true。
    let _fault = FaultEnvGuard::cleanup();
    let capture = test_log::begin();
    let receipt = adapter
        .write_atomic(
            &key(),
            b"v2",
            WriteOptions::new(Durability::ProcessCrashSafe),
        )
        .await;
    let logs = test_log::drain();
    drop(capture);

    let receipt = receipt.expect("post-Prepared fault returns committed receipt");
    assert_eq!(
        receipt.warning(),
        Some(crate::CommitWarning::JournalCleanupPending),
        "expected JournalCleanupPending warning"
    );

    let has_recovery = logs.iter().any(|(level, message)| {
        *level == log::Level::Warn
            && message == "blob_write recovery_pending journal_cleanup_pending"
    });
    assert!(
        has_recovery,
        "expected a recovery_pending Warn log, got {logs:?}"
    );

    // 清理。
    let _ = adapter
        .delete_all_generations(&key(), Default::default())
        .await;
    drop(adapter);
    let _ = std::fs::remove_dir_all(&root);
}
