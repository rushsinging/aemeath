use std::ffi::OsString;
use std::str::FromStr;
use std::sync::{Mutex, MutexGuard, OnceLock};

use super::FileSystemDatasetAdapter;
use crate::domain::{
    revision_member_digest, DatasetChangeSet, DatasetCommitVisibility, DatasetKey, DatasetMember,
    DatasetMemberChange, DatasetMemberReference, DatasetReadOutcome, DatasetRevision, Durability,
    SafePathSegment, StorageErrorKind, StorageNamespace, WriteOptions,
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

fn member_reference(
    revision: &DatasetRevision,
    name: &str,
    bytes: &[u8],
) -> DatasetMemberReference {
    DatasetMemberReference::from_manifest_member(
        revision.clone(),
        SafePathSegment::from_str(name).expect("safe member name"),
        bytes.len() as u64,
        revision_member_digest(bytes),
    )
}

#[tokio::test]
async fn incremental_commit_reuses_verified_member_and_publishes_complete_generation() {
    let root = root();
    let adapter = FileSystemDatasetAdapter::new(&root).expect("adapter init");
    let key = key();
    let empty_revision = adapter
        .read_manifest(&key)
        .await
        .expect("read empty manifest")
        .revision()
        .clone();
    adapter
        .commit_atomic(
            &key,
            &empty_revision,
            &[member("active", b"a1"), member("archive", b"z1")],
            WriteOptions::new(Durability::BestEffort),
        )
        .await
        .expect("seed generation");
    let current_revision = adapter
        .read_manifest(&key)
        .await
        .expect("read current manifest")
        .revision()
        .clone();
    let changes = DatasetChangeSet::new(
        current_revision.clone(),
        vec![DatasetMemberChange::Replace(member("active", b"a2"))],
        vec![member_reference(&current_revision, "archive", b"z1")],
    )
    .expect("valid incremental change set");

    adapter
        .commit_incremental(&key, &changes, WriteOptions::new(Durability::BestEffort))
        .await
        .expect("incremental commit");

    let requested = [
        SafePathSegment::from_str("active").expect("safe member name"),
        SafePathSegment::from_str("archive").expect("safe member name"),
    ];
    let DatasetReadOutcome::Found(read) = adapter
        .read_consistent(&key, &requested)
        .await
        .expect("read complete generation")
    else {
        panic!("complete generation should exist");
    };
    assert_eq!(
        read.members(),
        &[member("active", b"a2"), member("archive", b"z1")]
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;

        let dataset_path = root.join("memory").join("conv-log");
        let primary_inode =
            std::fs::metadata(dataset_path.join("primary").join("blobs").join("archive"))
                .expect("primary reused member metadata")
                .ino();
        let previous_inode =
            std::fs::metadata(dataset_path.join("previous").join("blobs").join("archive"))
                .expect("previous source member metadata")
                .ino();
        assert_eq!(
            primary_inode, previous_inode,
            "reused member must retain the immutable source inode instead of being rewritten"
        );
    }

    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test]
async fn incremental_commit_rejects_reused_member_when_primary_bytes_do_not_match_evidence() {
    let root = root();
    let adapter = FileSystemDatasetAdapter::new(&root).expect("adapter init");
    let key = key();
    let empty_revision = adapter
        .read_manifest(&key)
        .await
        .expect("read empty manifest")
        .revision()
        .clone();
    adapter
        .commit_atomic(
            &key,
            &empty_revision,
            &[member("active", b"a1"), member("archive", b"z1")],
            WriteOptions::new(Durability::BestEffort),
        )
        .await
        .expect("seed generation");
    let current_revision = adapter
        .read_manifest(&key)
        .await
        .expect("read current manifest")
        .revision()
        .clone();
    let invalid_reference = member_reference(&current_revision, "archive", b"other");
    let changes = DatasetChangeSet::new(
        current_revision,
        vec![DatasetMemberChange::Replace(member("active", b"a2"))],
        vec![invalid_reference],
    )
    .expect("domain-valid reference");

    let error = adapter
        .commit_incremental(&key, &changes, WriteOptions::new(Durability::BestEffort))
        .await
        .expect_err("mismatched reuse evidence must be rejected");

    assert!(matches!(
        error.kind(),
        StorageErrorKind::CorruptTransaction(_)
    ));
    let requested = [SafePathSegment::from_str("active").expect("safe member name")];
    let DatasetReadOutcome::Found(read) = adapter
        .read_consistent(&key, &requested)
        .await
        .expect("read unchanged primary")
    else {
        panic!("primary should remain readable");
    };
    assert_eq!(read.members(), &[member("active", b"a1")]);

    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test]
async fn incremental_commit_rejects_reused_member_missing_from_primary_manifest() {
    let root = root();
    let adapter = FileSystemDatasetAdapter::new(&root).expect("adapter init");
    let key = key();
    let empty_revision = adapter
        .read_manifest(&key)
        .await
        .expect("read empty manifest")
        .revision()
        .clone();
    adapter
        .commit_atomic(
            &key,
            &empty_revision,
            &[member("active", b"a1")],
            WriteOptions::new(Durability::BestEffort),
        )
        .await
        .expect("seed generation");
    let current_revision = adapter
        .read_manifest(&key)
        .await
        .expect("read current manifest")
        .revision()
        .clone();
    let changes = DatasetChangeSet::new(
        current_revision.clone(),
        Vec::new(),
        vec![member_reference(&current_revision, "archive", b"z1")],
    )
    .expect("domain-valid reference");

    let error = adapter
        .commit_incremental(&key, &changes, WriteOptions::new(Durability::BestEffort))
        .await
        .expect_err("missing source member must be rejected");

    assert!(matches!(
        error.kind(),
        StorageErrorKind::CorruptTransaction(_)
    ));

    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test]
async fn incremental_commit_removes_only_explicit_omitted_member() {
    let root = root();
    let adapter = FileSystemDatasetAdapter::new(&root).expect("adapter init");
    let key = key();
    let empty_revision = adapter
        .read_manifest(&key)
        .await
        .expect("read empty manifest")
        .revision()
        .clone();
    adapter
        .commit_atomic(
            &key,
            &empty_revision,
            &[
                member("active", b"a1"),
                member("archive", b"z1"),
                member("index", b"i1"),
            ],
            WriteOptions::new(Durability::BestEffort),
        )
        .await
        .expect("seed generation");
    let current_revision = adapter
        .read_manifest(&key)
        .await
        .expect("read current manifest")
        .revision()
        .clone();
    let changes = DatasetChangeSet::new(
        current_revision.clone(),
        Vec::new(),
        vec![
            member_reference(&current_revision, "active", b"a1"),
            member_reference(&current_revision, "index", b"i1"),
        ],
    )
    .expect("valid incremental change set")
    .with_removed_members(vec![
        SafePathSegment::from_str("archive").expect("safe member name")
    ])
    .expect("valid removal");

    adapter
        .commit_incremental(&key, &changes, WriteOptions::new(Durability::BestEffort))
        .await
        .expect("incremental removal");

    let manifest = adapter.read_manifest(&key).await.expect("read manifest");
    let names = manifest
        .members()
        .iter()
        .map(SafePathSegment::as_str)
        .collect::<Vec<_>>();
    assert_eq!(names, ["active", "index"]);

    let _ = std::fs::remove_dir_all(&root);
}

#[allow(
    clippy::await_holding_lock,
    reason = "故障环境变量是进程全局状态，测试必须在整个异步提交期间独占它"
)]
#[tokio::test(flavor = "current_thread")]
async fn incremental_commit_after_prepared_recovers_complete_generation() {
    let root = root();
    let adapter = FileSystemDatasetAdapter::new(&root).expect("adapter init");
    let key = key();
    let empty_revision = adapter
        .read_manifest(&key)
        .await
        .expect("read empty manifest")
        .revision()
        .clone();
    adapter
        .commit_atomic(
            &key,
            &empty_revision,
            &[member("active", b"a1"), member("archive", b"z1")],
            WriteOptions::new(Durability::BestEffort),
        )
        .await
        .expect("seed generation");
    let current_revision = adapter
        .read_manifest(&key)
        .await
        .expect("read current manifest")
        .revision()
        .clone();
    let changes = DatasetChangeSet::new(
        current_revision.clone(),
        vec![DatasetMemberChange::Replace(member("active", b"a2"))],
        vec![member_reference(&current_revision, "archive", b"z1")],
    )
    .expect("valid incremental change set");

    let fault = FaultEnvGuard::after_prepared();
    let receipt = adapter
        .commit_incremental(&key, &changes, WriteOptions::new(Durability::BestEffort))
        .await
        .expect("post-Prepared failure is committed");
    assert_eq!(
        receipt.visibility(),
        DatasetCommitVisibility::RecoveryPending
    );
    drop(fault);

    let requested = [
        SafePathSegment::from_str("active").expect("safe member name"),
        SafePathSegment::from_str("archive").expect("safe member name"),
    ];
    let DatasetReadOutcome::Found(read) = adapter
        .read_consistent(&key, &requested)
        .await
        .expect("next lock entry rolls generation forward")
    else {
        panic!("recovered generation should exist");
    };
    assert_eq!(
        read.members(),
        &[member("active", b"a2"), member("archive", b"z1")]
    );

    let _ = std::fs::remove_dir_all(&root);
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
