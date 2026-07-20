use std::str::FromStr;

use super::FileSystemDatasetAdapter;
use crate::domain::{
    DatasetCommitVisibility, DatasetKey, DatasetMember, DatasetRevision, Durability,
    SafePathSegment, StorageNamespace, WriteOptions,
};
use crate::test_log;
use crate::AtomicDatasetPort;

fn root() -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "aemeath-storage-dataset-log-{}",
        uuid::Uuid::new_v4()
    ))
}

fn key() -> DatasetKey {
    DatasetKey::new(
        StorageNamespace::Memory,
        vec![SafePathSegment::from_str("conv-log").unwrap()],
    )
    .unwrap()
}

fn member(name: &str, bytes: &[u8]) -> DatasetMember {
    DatasetMember::new(SafePathSegment::from_str(name).unwrap(), bytes.to_vec())
}

#[test]
fn init_success_emits_enter_then_ok() {
    let dir = tempfile::tempdir().expect("temp dir");
    test_log::begin();
    let result = FileSystemDatasetAdapter::new(dir.path());
    test_log::end();

    assert!(result.is_ok(), "construction should succeed");
    let logs = test_log::drain();
    let has_enter = logs.iter().any(|(_, message)| message.contains("enter"));
    let has_ok = logs
        .iter()
        .any(|(level, message)| *level <= log::Level::Info && message.contains("ok"));
    assert!(has_enter, "expected an 'enter' log line, got {logs:?}");
    assert!(
        has_ok,
        "expected an 'ok' log line at Info-or-lower, got {logs:?}"
    );
}

#[test]
fn init_failure_emits_enter_then_failed_at_error() {
    let file = tempfile::NamedTempFile::new().expect("temp file");
    test_log::begin();
    let result = FileSystemDatasetAdapter::new(file.path().join("subdir"));
    test_log::end();

    assert!(result.is_err(), "construction should fail");
    let logs = test_log::drain();
    let has_enter = logs.iter().any(|(_, message)| message.contains("enter"));
    let has_failed = logs
        .iter()
        .any(|(level, message)| *level == log::Level::Error && message.contains("failed"));
    assert!(has_enter, "expected an 'enter' log line, got {logs:?}");
    assert!(
        has_failed,
        "expected a 'failed' log line at Error level, got {logs:?}"
    );
}

#[tokio::test]
async fn commit_recovery_pending_emits_warn() {
    let root = root();
    let adapter = FileSystemDatasetAdapter::new(&root).expect("adapter init");
    let key = key();

    let expected: DatasetRevision = adapter
        .read_manifest(&key)
        .await
        .expect("read_manifest")
        .revision()
        .clone();
    adapter
        .commit_atomic(
            &key,
            &expected,
            &[member("active", b"a1")],
            WriteOptions::new(Durability::BestEffort),
        )
        .await
        .expect("first commit");

    let expected: DatasetRevision = adapter
        .read_manifest(&key)
        .await
        .expect("read_manifest")
        .revision()
        .clone();

    std::env::set_var("AEMEATH_STORAGE_DATASET_FAULT_POINT", "after_prepared");
    test_log::begin();
    let receipt = adapter
        .commit_atomic(
            &key,
            &expected,
            &[member("active", b"a2")],
            WriteOptions::new(Durability::BestEffort),
        )
        .await;
    test_log::end();
    std::env::remove_var("AEMEATH_STORAGE_DATASET_FAULT_POINT");

    let receipt = receipt.expect("post-Prepared fault returns committed receipt");
    assert_eq!(
        receipt.visibility(),
        DatasetCommitVisibility::RecoveryPending,
        "expected RecoveryPending visibility"
    );
    let logs = test_log::drain();
    assert!(
        logs.iter().any(|(level, message)| {
            *level == log::Level::Warn && message.contains("recovery_pending")
        }),
        "expected a recovery_pending Warn log, got {logs:?}"
    );

    drop(adapter);
    let _ = std::fs::remove_dir_all(&root);
}
