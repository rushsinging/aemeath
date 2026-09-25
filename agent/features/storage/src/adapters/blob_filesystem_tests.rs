use std::ffi::OsString;
use std::str::FromStr;
use std::sync::{Mutex, MutexGuard, OnceLock};

use super::FileSystemBlobAdapter;
use crate::domain::{Durability, SafePathSegment, StorageKey, StorageNamespace, WriteOptions};
use crate::ports::AtomicBlobPort;
use crate::test_log;

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

/// 按 project 分目录的 session 布局使用多段 StorageKey；`list_primary` 必须
/// 递归列出子目录内的 blob，同时不破坏平铺（单段）key 的枚举。
#[tokio::test(flavor = "current_thread")]
async fn list_primary_enumerates_nested_segment_keys_and_flat_keys() {
    let root = root();
    let adapter = FileSystemBlobAdapter::new(&root).expect("adapter init");

    let nested_key = StorageKey::new(
        StorageNamespace::Session,
        vec![
            SafePathSegment::from_str("project-dir-a").unwrap(),
            SafePathSegment::from_str("session-1").unwrap(),
        ],
    )
    .unwrap();
    let flat_key = StorageKey::new(
        StorageNamespace::Session,
        vec![SafePathSegment::from_str("flat-session").unwrap()],
    )
    .unwrap();

    adapter
        .write_atomic(
            &nested_key,
            b"nested",
            WriteOptions::new(Durability::ProcessCrashSafe),
        )
        .await
        .expect("nested write must succeed");
    adapter
        .write_atomic(
            &flat_key,
            b"flat",
            WriteOptions::new(Durability::ProcessCrashSafe),
        )
        .await
        .expect("flat write must succeed");

    let listed = adapter
        .list_primary(StorageNamespace::Session)
        .await
        .expect("list must succeed");
    let listed_keys: Vec<&StorageKey> = listed.iter().map(|entry| entry.key()).collect();
    assert!(
        listed_keys.contains(&&nested_key),
        "嵌套 project 段 key 必须被列出：{listed_keys:?}"
    );
    assert!(
        listed_keys.contains(&&flat_key),
        "平铺 key 必须继续被列出：{listed_keys:?}"
    );

    drop(adapter);
    let _ = std::fs::remove_dir_all(&root);
}
