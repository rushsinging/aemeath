use std::ffi::OsString;
use std::str::FromStr;
use std::sync::{Mutex, MutexGuard, OnceLock};

use super::FileSystemDatasetAdapter;
use crate::domain::{
    DatasetCommitVisibility, DatasetKey, DatasetMember, DatasetRevision, Durability,
    SafePathSegment, StorageNamespace, WriteOptions,
};
use crate::test_log;
use crate::AtomicDatasetPort;

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
    fn after_prepared() -> Self {
        let lock = fault_env_lock();
        let previous = std::env::var_os("AEMEATH_STORAGE_DATASET_FAULT_POINT");
        std::env::set_var("AEMEATH_STORAGE_DATASET_FAULT_POINT", "after_prepared");
        Self {
            previous,
            _lock: lock,
        }
    }
}

impl Drop for FaultEnvGuard {
    fn drop(&mut self) {
        match self.previous.take() {
            Some(value) => std::env::set_var("AEMEATH_STORAGE_DATASET_FAULT_POINT", value),
            None => std::env::remove_var("AEMEATH_STORAGE_DATASET_FAULT_POINT"),
        }
    }
}

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
    let capture = test_log::begin();
    let result = FileSystemDatasetAdapter::new(dir.path());
    drop(capture);

    assert!(result.is_ok(), "construction should succeed");
    let logs = test_log::drain();
    assert!(!logs.is_empty(), "adapter initialization must emit logs");
    let has_enter = logs.iter().any(|(level, message)| {
        *level == log::Level::Debug && message == "dataset_adapter init enter"
    });
    let has_ok = logs
        .iter()
        .any(|(level, message)| *level == log::Level::Info && message == "dataset_adapter init ok");
    assert!(has_enter, "expected an 'enter' log line, got {logs:?}");
    assert!(has_ok, "expected an Info-level 'ok' log line, got {logs:?}");
}

#[test]
fn init_failure_emits_enter_then_failed_at_error() {
    let file = tempfile::NamedTempFile::new().expect("temp file");
    let capture = test_log::begin();
    let result = FileSystemDatasetAdapter::new(file.path().join("subdir"));
    drop(capture);

    assert!(result.is_err(), "construction should fail");
    let logs = test_log::drain();
    assert!(!logs.is_empty(), "failed initialization must emit logs");
    let has_enter = logs.iter().any(|(level, message)| {
        *level == log::Level::Debug && message == "dataset_adapter init enter"
    });
    let has_failed = logs.iter().any(|(level, message)| {
        *level == log::Level::Error && message == "dataset_adapter init failed"
    });
    assert!(has_enter, "expected an 'enter' log line, got {logs:?}");
    assert!(
        has_failed,
        "expected a 'failed' log line at Error level, got {logs:?}"
    );
}

#[allow(
    clippy::await_holding_lock,
    reason = "故障环境变量是进程全局状态，测试必须在整个异步提交期间独占它"
)]
#[tokio::test(flavor = "current_thread")]
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

    let _fault = FaultEnvGuard::after_prepared();
    let capture = test_log::begin();
    let receipt = adapter
        .commit_atomic(
            &key,
            &expected,
            &[member("active", b"a2")],
            WriteOptions::new(Durability::BestEffort),
        )
        .await;
    let logs = test_log::drain();
    drop(capture);

    let receipt = receipt.expect("post-Prepared fault returns committed receipt");
    assert_eq!(
        receipt.visibility(),
        DatasetCommitVisibility::RecoveryPending,
        "expected RecoveryPending visibility"
    );
    assert!(
        logs.iter().any(|(level, message)| {
            *level == log::Level::Warn && message == "dataset_commit recovery_pending"
        }),
        "expected a recovery_pending Warn log, got {logs:?}"
    );

    drop(adapter);
    let _ = std::fs::remove_dir_all(&root);
}
