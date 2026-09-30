use std::str::FromStr;

use super::{BlobFaultInjector, FileSystemBlobAdapter};
use crate::domain::{
    DurabilityData, SafePathSegmentData, StorageKeyData, StorageNamespaceData, WriteOptionsData,
};
use crate::ports::AtomicBlobPort;
use crate::test_log;

fn root() -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "aemeath-storage-blob-recovery-log-{}",
        uuid::Uuid::new_v4()
    ))
}

fn key() -> StorageKeyData {
    StorageKeyData::new(
        StorageNamespaceData::Session,
        vec![SafePathSegmentData::from_str("log-test").unwrap()],
    )
    .unwrap()
}

/// 当 write_atomic 越过逻辑提交点（crossed_commit = true）后在 cleanup 阶段
/// 故障时，返回 committed JournalCleanupPending 收据——必须同时 emit 一条
/// Warn 级 recovery_pending 日志，且不泄露 key / 路径。
#[tokio::test(flavor = "current_thread")]
async fn cleanup_fault_emits_recovery_pending_warn() {
    let root = root();
    let mut adapter = FileSystemBlobAdapter::new(&root).expect("adapter init");

    // 第一次写入：成功建立 primary。
    adapter
        .write_atomic(
            &key(),
            b"v1",
            WriteOptionsData::new(DurabilityData::ProcessCrashSafe),
        )
        .await
        .expect("first write must succeed");

    // 在 cleanup（post-commit）注入故障：crossed_commit 已为 true。
    adapter.set_faults(BlobFaultInjector::requested("cleanup"));
    let capture = test_log::begin();
    let receipt = adapter
        .write_atomic(
            &key(),
            b"v2",
            WriteOptionsData::new(DurabilityData::ProcessCrashSafe),
        )
        .await;
    let logs = test_log::drain();
    drop(capture);

    let receipt = receipt.expect("post-Prepared fault returns committed receipt");
    assert_eq!(
        receipt.warning(),
        Some(crate::CommitWarningData::JournalCleanupPending),
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

/// 按 project 分目录的 session 布局使用多段 StorageKeyData；`list_primary` 必须
/// 递归列出子目录内的 blob，同时不破坏平铺（单段）key 的枚举。
#[tokio::test(flavor = "current_thread")]
async fn list_primary_enumerates_nested_segment_keys_and_flat_keys() {
    let root = root();
    let adapter = FileSystemBlobAdapter::new(&root).expect("adapter init");

    let nested_key = StorageKeyData::new(
        StorageNamespaceData::Session,
        vec![
            SafePathSegmentData::from_str("project-dir-a").unwrap(),
            SafePathSegmentData::from_str("session-1").unwrap(),
        ],
    )
    .unwrap();
    let flat_key = StorageKeyData::new(
        StorageNamespaceData::Session,
        vec![SafePathSegmentData::from_str("flat-session").unwrap()],
    )
    .unwrap();

    adapter
        .write_atomic(
            &nested_key,
            b"nested",
            WriteOptionsData::new(DurabilityData::ProcessCrashSafe),
        )
        .await
        .expect("nested write must succeed");
    adapter
        .write_atomic(
            &flat_key,
            b"flat",
            WriteOptionsData::new(DurabilityData::ProcessCrashSafe),
        )
        .await
        .expect("flat write must succeed");

    let listed = adapter
        .list_primary(StorageNamespaceData::Session)
        .await
        .expect("list must succeed");
    let listed_keys: Vec<&StorageKeyData> = listed.iter().map(|entry| entry.key()).collect();
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

#[tokio::test]
async fn fault_injection_scopes_to_the_adapter_that_requested_it() {
    // 根因修复的行为表达：故障注入是 adapter 实例状态，不是进程全局 env。
    let clean_root = root();
    let injected_root = root();
    let clean = FileSystemBlobAdapter::new(&clean_root).expect("clean adapter");
    let mut injected = FileSystemBlobAdapter::new(&injected_root).expect("injected adapter");
    injected.set_faults(BlobFaultInjector::requested("cleanup"));

    clean
        .write_atomic(
            &key(),
            b"v1",
            WriteOptionsData::new(DurabilityData::ProcessCrashSafe),
        )
        .await
        .expect("clean adapter write must not be poisoned by sibling injection");

    injected
        .write_atomic(
            &key(),
            b"v1",
            WriteOptionsData::new(DurabilityData::ProcessCrashSafe),
        )
        .await
        .expect("first write on injected adapter establishes primary");
    let receipt = injected
        .write_atomic(
            &key(),
            b"v2",
            WriteOptionsData::new(DurabilityData::ProcessCrashSafe),
        )
        .await
        .expect("injected write returns degraded receipt");
    assert_eq!(
        receipt.warning(),
        Some(crate::CommitWarningData::JournalCleanupPending),
        "显式注入必须命中目标 adapter"
    );

    let _ = std::fs::remove_dir_all(&clean_root);
    let _ = std::fs::remove_dir_all(&injected_root);
}
