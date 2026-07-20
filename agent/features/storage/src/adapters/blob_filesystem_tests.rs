use std::str::FromStr;

use super::FileSystemBlobAdapter;
use crate::domain::{Durability, SafePathSegment, StorageKey, StorageNamespace, WriteOptions};
use crate::test_log;
use crate::AtomicBlobPort;

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
#[tokio::test]
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
    std::env::set_var("AEMEATH_STORAGE_FAULT_POINT", "cleanup");
    test_log::begin();
    let receipt = adapter
        .write_atomic(
            &key(),
            b"v2",
            WriteOptions::new(Durability::ProcessCrashSafe),
        )
        .await;
    test_log::end();
    std::env::remove_var("AEMEATH_STORAGE_FAULT_POINT");

    let receipt = receipt.expect("post-Prepared fault returns committed receipt");
    assert_eq!(
        receipt.warning(),
        Some(crate::CommitWarning::JournalCleanupPending),
        "expected JournalCleanupPending warning"
    );

    let logs = test_log::drain();
    let has_recovery = logs
        .iter()
        .any(|(level, m)| *level == log::Level::Warn && m.contains("recovery_pending"));
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
