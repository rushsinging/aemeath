use std::ffi::OsString;
use std::str::FromStr;
use std::sync::{Mutex, MutexGuard, OnceLock};

use super::FileSystemDatasetAdapter;
use crate::domain::{
    revision_member_digest, DatasetChangeSet, DatasetCommitVisibility, DatasetKey, DatasetManifest,
    DatasetMember, DatasetMemberChange, DatasetMemberReference, DatasetReadOutcome,
    DatasetRevision, Durability, SafePathSegment, StorageErrorKind, StorageNamespace, WriteOptions,
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
async fn incremental_commit_persists_only_changed_member_content() {
    let root = root();
    let adapter = FileSystemDatasetAdapter::new(&root).expect("adapter init");
    let key = key();
    let empty_revision = adapter
        .read_manifest(&key)
        .await
        .expect("read empty manifest")
        .revision()
        .clone();
    let historical_members = (0..64)
        .map(|index| {
            member(
                &format!("history-{index:03}"),
                format!("payload-{index}").as_bytes(),
            )
        })
        .collect::<Vec<_>>();
    adapter
        .commit_atomic(
            &key,
            &empty_revision,
            &historical_members,
            WriteOptions::new(Durability::BestEffort),
        )
        .await
        .expect("seed generation");
    let current_manifest = adapter
        .read_manifest(&key)
        .await
        .expect("read current manifest");
    let current_revision = current_manifest.revision().clone();
    let changed_name = "history-000";
    let changes = DatasetChangeSet::new(
        current_revision.clone(),
        vec![DatasetMemberChange::Replace(member(
            changed_name,
            b"changed",
        ))],
        historical_members
            .iter()
            .filter(|historical_member| historical_member.name().as_str() != changed_name)
            .map(|historical_member| {
                member_reference(
                    &current_revision,
                    historical_member.name().as_str(),
                    historical_member.bytes(),
                )
            })
            .collect(),
    )
    .expect("valid incremental change set");
    let dataset_path = root.join("memory").join("conv-log");
    adapter
        .commit_incremental(&key, &changes, WriteOptions::new(Durability::BestEffort))
        .await
        .expect("first incremental commit migrates legacy member content");
    let migrated_manifest = adapter
        .read_manifest(&key)
        .await
        .expect("read migrated manifest");
    let migrated_revision = migrated_manifest.revision().clone();
    let second_changes = DatasetChangeSet::new(
        migrated_revision.clone(),
        vec![DatasetMemberChange::Replace(member(
            changed_name,
            b"changed-again",
        ))],
        historical_members
            .iter()
            .filter(|historical_member| historical_member.name().as_str() != changed_name)
            .map(|historical_member| {
                member_reference(
                    &migrated_revision,
                    historical_member.name().as_str(),
                    historical_member.bytes(),
                )
            })
            .collect(),
    )
    .expect("valid post-migration incremental change set");
    let content_files_before = dataset_member_content_file_count(&dataset_path);

    adapter
        .commit_incremental(
            &key,
            &second_changes,
            WriteOptions::new(Durability::BestEffort),
        )
        .await
        .expect("incremental commit");

    let content_files_after = dataset_member_content_file_count(&dataset_path);
    assert_eq!(
        content_files_after - content_files_before,
        1,
        "replacing one member must persist exactly one new immutable content file regardless of historical member count"
    );
    assert!(
        !dataset_path.join("primary/blobs").exists(),
        "primary generation must reference shared member content instead of retaining a complete blobs tree"
    );
    assert!(
        !dataset_path.join("previous/blobs").exists(),
        "previous generation must reference shared member content instead of retaining a complete blobs tree"
    );

    let _ = std::fs::remove_dir_all(&root);
}

fn dataset_member_content_file_count(dataset_path: &std::path::Path) -> usize {
    let member_store = dataset_path.join("members");
    std::fs::read_dir(member_store)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .filter(|entry| entry.file_type().is_ok_and(|file_type| file_type.is_file()))
                .count()
        })
        .unwrap_or_default()
}

#[tokio::test]
async fn legacy_manifest_without_member_evidence_reads_and_migrates_on_incremental_commit() {
    let root = root();
    let dataset_path = root.join("memory").join("conv-log");
    let primary_blobs = dataset_path.join("primary").join("blobs");
    std::fs::create_dir_all(&primary_blobs).expect("create legacy primary blobs");
    let active = member("active", b"a1");
    let archive = member("archive", b"z1");
    let legacy_manifest = DatasetManifest::new(vec![active.clone(), archive.clone()])
        .expect("legacy manifest domain value");
    std::fs::write(primary_blobs.join("active"), active.bytes()).expect("write legacy active");
    std::fs::write(primary_blobs.join("archive"), archive.bytes()).expect("write legacy archive");
    std::fs::write(
        dataset_path.join("primary").join("manifest.json"),
        serde_json::json!({
            "修订号": super::proto::encode_revision(legacy_manifest.revision().as_bytes()),
            "成员集合": ["active", "archive"]
        })
        .to_string(),
    )
    .expect("write legacy manifest without member evidence");

    let adapter = FileSystemDatasetAdapter::new(&root).expect("adapter init");
    let requested = [
        SafePathSegment::from_str("active").expect("safe member name"),
        SafePathSegment::from_str("archive").expect("safe member name"),
    ];
    let DatasetReadOutcome::Found(read) = adapter
        .read_consistent(&key(), &requested)
        .await
        .expect("legacy manifest must remain readable")
    else {
        panic!("legacy generation should be found");
    };
    assert_eq!(read.members(), &[active.clone(), archive.clone()]);

    let changes = DatasetChangeSet::new(
        legacy_manifest.revision().clone(),
        vec![DatasetMemberChange::Replace(member("active", b"a2"))],
        vec![member_reference(
            legacy_manifest.revision(),
            "archive",
            archive.bytes(),
        )],
    )
    .expect("valid migration change set");
    adapter
        .commit_incremental(&key(), &changes, WriteOptions::new(Durability::BestEffort))
        .await
        .expect("first incremental commit migrates legacy manifest");

    assert!(!dataset_path.join("primary/blobs").exists());
    assert!(!dataset_path.join("previous/blobs").exists());
    assert_eq!(dataset_member_content_file_count(&dataset_path), 2);
    let persisted_manifest: serde_json::Value = serde_json::from_slice(
        &std::fs::read(dataset_path.join("primary/manifest.json")).expect("read migrated manifest"),
    )
    .expect("decode migrated manifest");
    assert_eq!(
        persisted_manifest["成员证据"]
            .as_array()
            .expect("migrated member evidence")
            .len(),
        2
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test]
async fn legacy_manifest_with_revision_evidence_but_without_content_digests_reads_and_migrates() {
    let root = root();
    let dataset_path = root.join("memory").join("conv-log");
    let primary_blobs = dataset_path.join("primary").join("blobs");
    std::fs::create_dir_all(&primary_blobs).expect("create legacy primary blobs");
    let active = member("active", b"a1");
    let archive = member("archive", b"z1");
    let legacy_manifest = DatasetManifest::new(vec![active.clone(), archive.clone()])
        .expect("legacy manifest domain value");
    std::fs::write(primary_blobs.join("active"), active.bytes()).expect("write legacy active");
    std::fs::write(primary_blobs.join("archive"), archive.bytes()).expect("write legacy archive");
    std::fs::write(
        dataset_path.join("primary").join("manifest.json"),
        serde_json::json!({
            "修订号": super::proto::encode_revision(legacy_manifest.revision().as_bytes()),
            "成员集合": ["active", "archive"],
            "成员证据": [
                {
                    "名称": "active",
                    "字节数": active.bytes().len(),
                    "修订摘要": super::proto::revision_member_digest_hex(active.bytes())
                },
                {
                    "名称": "archive",
                    "字节数": archive.bytes().len(),
                    "修订摘要": super::proto::revision_member_digest_hex(archive.bytes())
                }
            ]
        })
        .to_string(),
    )
    .expect("write legacy manifest without content digests");

    let adapter = FileSystemDatasetAdapter::new(&root).expect("adapter init");
    let requested = [
        SafePathSegment::from_str("active").expect("safe member name"),
        SafePathSegment::from_str("archive").expect("safe member name"),
    ];
    let DatasetReadOutcome::Found(read) = adapter
        .read_consistent(&key(), &requested)
        .await
        .expect("legacy manifest with revision evidence must remain readable")
    else {
        panic!("legacy generation should be found");
    };
    assert_eq!(read.members(), &[active.clone(), archive.clone()]);

    let changes = DatasetChangeSet::new(
        legacy_manifest.revision().clone(),
        vec![DatasetMemberChange::Replace(member("active", b"a2"))],
        vec![member_reference(
            legacy_manifest.revision(),
            "archive",
            archive.bytes(),
        )],
    )
    .expect("valid migration change set");
    adapter
        .commit_incremental(&key(), &changes, WriteOptions::new(Durability::BestEffort))
        .await
        .expect("first incremental commit migrates legacy manifest");

    assert!(!dataset_path.join("primary/blobs").exists());
    assert!(!dataset_path.join("previous/blobs").exists());
    assert_eq!(dataset_member_content_file_count(&dataset_path), 2);
    let persisted_manifest: serde_json::Value = serde_json::from_slice(
        &std::fs::read(dataset_path.join("primary/manifest.json")).expect("read migrated manifest"),
    )
    .expect("decode migrated manifest");
    assert!(persisted_manifest["成员证据"]
        .as_array()
        .expect("migrated member evidence")
        .iter()
        .all(|member| member["内容摘要"]
            .as_str()
            .is_some_and(|digest| !digest.is_empty())));

    let _ = std::fs::remove_dir_all(&root);
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

    let dataset_path = root.join("memory").join("conv-log");
    let shared_members = dataset_path.join("members");
    assert_eq!(
        std::fs::read_dir(&shared_members)
            .expect("shared member store")
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_ok_and(|file_type| file_type.is_file()))
            .count(),
        3,
        "two initial members plus one changed value must occupy three immutable content files"
    );
    assert!(!dataset_path.join("primary/blobs").exists());
    assert!(!dataset_path.join("previous/blobs").exists());

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
