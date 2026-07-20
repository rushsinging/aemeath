//! #983 crash/recovery contract for the filesystem `AtomicDatasetPort` adapter.
//!
//! Most assertions use the published port. Directory inspection is deliberately
//! limited to transaction evidence and the documented private on-disk boundary.
//! The child-process cases reserve a private, tests-only fault seam:
//!
//! - `AEMEATH_STORAGE_DATASET_FAULT_POINT=after_prepared`
//! - `AEMEATH_STORAGE_DATASET_FAULT_POINT=after_member_publish:<name>`
//! - `AEMEATH_STORAGE_DATASET_FAULT_POINT=promote_after_prepared`
//! - `AEMEATH_STORAGE_DATASET_FAULT_POINT=promote_after_primary_to_swap`
//! - `AEMEATH_STORAGE_DATASET_FAULT_POINT=promote_after_previous_to_primary`
//! - `AEMEATH_STORAGE_DATASET_FAULT_ABORT=1` aborts at that point.
//!
//! Without `...FAULT_ABORT`, a fault after the durable Prepared record must be
//! converted into a committed `RecoveryPending` receipt. These environment
//! variables are a test-driver detail, not part of `AtomicDatasetPort`.

use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::str::FromStr;
use std::time::{Duration, Instant};

use fs2::FileExt;
use sha2::{Digest, Sha256};
use storage::{
    AtomicDatasetPort, CommitWarning, DatasetCommitVisibility, DatasetKey, DatasetMember,
    DatasetReadOutcome, Durability, FileSystemDatasetAdapter, QuarantineDisposition,
    SafePathSegment, StorageError, StorageErrorKind, StorageNamespace, TransactionScope,
    WriteOptions,
};
use uuid::Uuid;

const HELPER_MODE: &str = "AEMEATH_DATASET_CHILD_MODE";
const HELPER_ROOT: &str = "AEMEATH_DATASET_CHILD_ROOT";
const HELPER_READY: &str = "AEMEATH_DATASET_CHILD_READY";
const HELPER_RESULT: &str = "AEMEATH_DATASET_CHILD_RESULT";
const FAULT_POINT: &str = "AEMEATH_STORAGE_DATASET_FAULT_POINT";
const FAULT_ABORT: &str = "AEMEATH_STORAGE_DATASET_FAULT_ABORT";

fn unique_root(case: &str) -> PathBuf {
    std::env::temp_dir().join(format!("aemeath-dataset-crash-{case}-{}", Uuid::new_v4()))
}

fn key_named(value: &str) -> DatasetKey {
    DatasetKey::new(
        StorageNamespace::Memory,
        vec![SafePathSegment::from_str(value).expect("valid dataset segment")],
    )
    .expect("valid dataset key")
}

fn key() -> DatasetKey {
    key_named("conversation-1")
}

fn name(value: &str) -> SafePathSegment {
    SafePathSegment::from_str(value).expect("valid member name")
}

fn member(value: &str, bytes: &[u8]) -> DatasetMember {
    DatasetMember::new(name(value), bytes.to_vec())
}

fn old_members() -> Vec<DatasetMember> {
    vec![
        member("active", b"old-active"),
        member("archive", b"old-archive"),
    ]
}

fn new_members() -> Vec<DatasetMember> {
    vec![
        member("active", b"new-active"),
        member("archive", b"new-archive"),
    ]
}

fn options() -> WriteOptions {
    WriteOptions::new(Durability::ProcessCrashSafe)
}

fn dataset_dir(root: &Path, dataset: &str) -> PathBuf {
    root.join("memory").join(dataset)
}

fn adapter(root: &Path) -> FileSystemDatasetAdapter {
    FileSystemDatasetAdapter::new(root).expect("adapter root must initialize")
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Runtime::new().expect("runtime must initialize")
}

fn seed(root: &Path, dataset: &DatasetKey, members: &[DatasetMember]) -> storage::DatasetRevision {
    let adapter = adapter(root);
    let runtime = runtime();
    let expected = runtime
        .block_on(adapter.read_manifest(dataset))
        .expect("manifest read must succeed")
        .revision()
        .clone();
    runtime
        .block_on(adapter.commit_atomic(dataset, &expected, members, options()))
        .expect("seed commit must succeed")
        .revision()
        .clone()
}

fn spawn_fault_child_mode(
    root: &Path,
    mode: &str,
    point: &str,
    abort: bool,
) -> std::process::ExitStatus {
    let mut command = Command::new(std::env::current_exe().expect("test executable"));
    command
        .arg("--exact")
        .arg("dataset_child_runs_transaction")
        .arg("--nocapture")
        .env(HELPER_MODE, mode)
        .env(HELPER_ROOT, root)
        .env(FAULT_POINT, point)
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if abort {
        command.env(FAULT_ABORT, "1");
    }
    command.status().expect("fault child must launch")
}

fn spawn_fault_child(root: &Path, point: &str, abort: bool) -> std::process::ExitStatus {
    spawn_fault_child_mode(root, "commit", point, abort)
}

fn spawn_promote_fault_child(root: &Path, point: &str) -> std::process::ExitStatus {
    spawn_fault_child_mode(root, "promote", point, true)
}

fn read_generation_pair(root: &Path, previous: bool) -> (Vec<u8>, Vec<u8>) {
    let adapter = adapter(root);
    let runtime = runtime();
    let outcome = if previous {
        runtime.block_on(adapter.read_previous(&key(), &[name("active"), name("archive")]))
    } else {
        runtime.block_on(adapter.read_consistent(&key(), &[name("active"), name("archive")]))
    }
    .expect("reopen must recover before reading");
    let DatasetReadOutcome::Found(read) = outcome else {
        panic!("recovered generation must contain both requested members");
    };
    let mut values = read
        .members()
        .iter()
        .map(|member| (member.name().as_str(), member.bytes().to_vec()))
        .collect::<Vec<_>>();
    values.sort_by_key(|(member_name, _)| *member_name);
    (values[0].1.clone(), values[1].1.clone())
}

fn read_pair(root: &Path) -> (Vec<u8>, Vec<u8>) {
    read_generation_pair(root, false)
}

fn transaction_artifacts(root: &Path, dataset: &str) -> Vec<String> {
    let dir = dataset_dir(root, dataset);
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .map(|entry| {
            entry
                .expect("directory entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .filter(|entry| {
            entry == "journal.json"
                || entry == "previous.next"
                || entry.starts_with(".stage-")
                || entry.starts_with(".journal-")
                || entry.starts_with(".swap-")
        })
        .collect()
}

fn stage_dir(root: &Path) -> PathBuf {
    std::fs::read_dir(dataset_dir(root, "conversation-1"))
        .expect("dataset directory")
        .map(|entry| entry.expect("directory entry").path())
        .find(|path| {
            path.file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with(".stage-"))
        })
        .expect("Prepared transaction must retain its stage")
}

fn assert_corrupt_and_quarantined(error: StorageError, root: &Path) {
    let StorageErrorKind::CorruptTransaction(corruption) = error.kind() else {
        panic!("recovery contradiction must outrank ordinary I/O/CAS errors: {error:?}");
    };
    assert_eq!(corruption.scope(), TransactionScope::Dataset);
    assert_eq!(
        corruption.quarantine_disposition(),
        QuarantineDisposition::EvidenceQuarantined,
        "typed corruption must report that transaction evidence was quarantined"
    );
    assert!(
        std::fs::read_dir(dataset_dir(root, "conversation-1"))
            .expect("dataset directory")
            .any(|entry| entry
                .expect("directory entry")
                .file_name()
                .to_string_lossy()
                .contains(".corrupt.")),
        "journal/stage/published evidence must be retained under a corruption quarantine name"
    );
}

#[test]
fn dataset_child_runs_transaction() {
    let Some(mode) = std::env::var_os(HELPER_MODE) else {
        return;
    };
    if mode != std::ffi::OsStr::new("commit") && mode != std::ffi::OsStr::new("promote") {
        return;
    }
    let root = PathBuf::from(std::env::var_os(HELPER_ROOT).expect("child root"));
    let adapter = adapter(&root);
    let runtime = runtime();
    let outcome = if mode == std::ffi::OsStr::new("promote") {
        runtime.block_on(adapter.promote_previous(&key()))
    } else {
        let expected = runtime
            .block_on(adapter.read_manifest(&key()))
            .expect("child manifest read")
            .revision()
            .clone();
        runtime.block_on(adapter.commit_atomic(&key(), &expected, &new_members(), options()))
    };

    if let Some(result) = std::env::var_os(HELPER_RESULT) {
        let label = match outcome {
            Ok(receipt)
                if receipt.visibility() == DatasetCommitVisibility::RecoveryPending
                    && receipt.warning() == Some(CommitWarning::MemberPublishRecoveryPending) =>
            {
                "recovery-pending"
            }
            Ok(receipt) if receipt.visibility() == DatasetCommitVisibility::Visible => "visible",
            Ok(_) => "wrong-receipt",
            Err(_) => "error",
        };
        std::fs::write(result, label).expect("write child result");
    }
}

#[test]
fn lock_child_holds_dataset_os_lock() {
    if std::env::var_os(HELPER_MODE).as_deref() != Some(std::ffi::OsStr::new("lock")) {
        return;
    }
    let root = PathBuf::from(std::env::var_os(HELPER_ROOT).expect("child root"));
    let ready = PathBuf::from(std::env::var_os(HELPER_READY).expect("ready path"));
    let release = PathBuf::from(std::env::var_os(HELPER_RESULT).expect("release path"));
    let lock = dataset_dir(&root, "conversation-1").join("dataset.lock");
    std::fs::create_dir_all(lock.parent().expect("lock parent")).expect("create dataset dir");
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(lock)
        .expect("open lock file");
    file.lock_exclusive().expect("acquire child lock");
    std::fs::write(ready, b"ready").expect("signal lock acquisition");
    let deadline = Instant::now() + Duration::from_secs(5);
    while !release.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(release.exists(), "parent must release the dataset lock");
}

fn locked_child(root: &Path) -> (std::process::Child, PathBuf, PathBuf) {
    let ready = root.join("lock-ready");
    let release = root.join("lock-release");
    let child = Command::new(std::env::current_exe().expect("test executable"))
        .arg("--exact")
        .arg("lock_child_holds_dataset_os_lock")
        .arg("--nocapture")
        .env(HELPER_MODE, "lock")
        .env(HELPER_ROOT, root)
        .env(HELPER_READY, &ready)
        .env(HELPER_RESULT, &release)
        .stdout(Stdio::null())
        .spawn()
        .expect("lock child must launch");
    let deadline = Instant::now() + Duration::from_secs(5);
    while !ready.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(ready.exists(), "child must acquire the real OS lock");
    (child, ready, release)
}

#[test]
fn same_dataset_is_serialized_across_processes() {
    let root = unique_root("same-lock");
    let (mut child, _, release) = locked_child(&root);
    let done = root.join("same-lock-done");
    let started = root.join("same-lock-started");
    let root_for_read = root.clone();
    let done_for_read = done.clone();
    let started_for_read = started.clone();
    let reader = std::thread::spawn(move || {
        std::fs::write(started_for_read, b"started").expect("signal reader start");
        runtime()
            .block_on(adapter(&root_for_read).read_manifest(&key()))
            .expect("read after lock release");
        std::fs::write(done_for_read, b"done").expect("signal reader completion");
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    while !started.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(started.exists(), "same-dataset reader must start");
    assert!(
        !done.exists(),
        "same dataset must remain blocked while the helper owns its lock"
    );
    std::fs::write(release, b"release").expect("release dataset lock");
    assert!(child.wait().expect("wait child").success());
    reader.join().expect("join same-dataset reader");
    assert!(done.exists(), "same dataset must finish after lock release");
    std::fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn different_datasets_do_not_block_each_other() {
    let root = unique_root("different-lock");
    let (mut child, _, release) = locked_child(&root);
    runtime()
        .block_on(adapter(&root).read_manifest(&key_named("conversation-2")))
        .expect("independent dataset read must finish while another dataset is locked");
    std::fs::write(release, b"release").expect("release dataset lock");
    assert!(child.wait().expect("wait child").success());
    std::fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn prepared_durable_then_child_abort_rolls_forward_complete_new_generation() {
    let root = unique_root("prepared-abort");
    seed(&root, &key(), &old_members());
    let status = spawn_fault_child(&root, "after_prepared", true);
    assert!(
        !status.success(),
        "tests-only fault must abort after durable Prepared"
    );
    assert_eq!(
        read_pair(&root),
        (b"new-active".to_vec(), b"new-archive".to_vec())
    );
    assert!(transaction_artifacts(&root, "conversation-1").is_empty());
    std::fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn abort_after_one_member_publish_never_exposes_a_mixed_generation() {
    let root = unique_root("one-member-abort");
    seed(&root, &key(), &old_members());
    let status = spawn_fault_child(&root, "after_member_publish:active", true);
    assert!(
        !status.success(),
        "tests-only fault must abort after one member publish"
    );
    let pair = read_pair(&root);
    assert_eq!(
        pair,
        (b"new-active".to_vec(), b"new-archive".to_vec()),
        "after_member_publish is post-Prepared: reopen must finish the committed new generation, never roll back to the old one"
    );
    assert!(transaction_artifacts(&root, "conversation-1").is_empty());
    std::fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn stale_cas_creates_no_transaction_artifacts() {
    let root = unique_root("stale-cas");
    let adapter = adapter(&root);
    let runtime = runtime();
    let stale = runtime
        .block_on(adapter.read_manifest(&key()))
        .expect("initial manifest")
        .revision()
        .clone();
    seed(&root, &key(), &old_members());
    let error = runtime
        .block_on(adapter.commit_atomic(&key(), &stale, &new_members(), options()))
        .expect_err("stale CAS must fail before transaction preparation");
    assert_eq!(error.kind(), &StorageErrorKind::ConcurrentWrite);
    assert!(
        transaction_artifacts(&root, "conversation-1").is_empty(),
        "CAS rejection must happen before stage/journal/previous.next creation"
    );
    assert_eq!(
        read_pair(&root),
        (b"old-active".to_vec(), b"old-archive".to_vec())
    );
    std::fs::remove_dir_all(root).expect("cleanup");
}

fn journal_json(members: &str, old_revision: &str, new_revision: &str) -> String {
    format!(
        r#"{{"随机数":"testnonce","操作":"完整替换提交","旧修订号":"{old_revision}","新修订号":"{new_revision}","成员集合":{members},"阶段":"已准备"}}"#
    )
}

fn rewrite_journal(root: &Path, rewrite: impl FnOnce(&mut serde_json::Value)) {
    let path = dataset_dir(root, "conversation-1").join("journal.json");
    let bytes = std::fs::read(&path).expect("Prepared journal must exist");
    let mut journal: serde_json::Value =
        serde_json::from_slice(&bytes).expect("fault-created journal must be valid JSON");
    rewrite(&mut journal);
    std::fs::write(
        path,
        serde_json::to_vec(&journal).expect("modified journal must remain valid JSON"),
    )
    .expect("replace journal with structurally valid contradiction");
}

fn other_hex64(current: &str, digit: char) -> String {
    let candidate = digit.to_string().repeat(64);
    if candidate == current {
        (if digit == 'a' { 'b' } else { 'a' })
            .to_string()
            .repeat(64)
    } else {
        candidate
    }
}

fn hex_sha256(bytes: impl AsRef<[u8]>) -> String {
    Sha256::digest(bytes.as_ref())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn revision_member_digest(bytes: &[u8]) -> String {
    let mut input = b"aemeath.storage.dataset.member.bytes.v1\0".to_vec();
    input.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
    input.extend_from_slice(bytes);
    hex_sha256(input)
}

fn decode_hex32(value: &str) -> [u8; 32] {
    assert_eq!(value.len(), 64, "test revision digest must be valid hex32");
    let mut decoded = [0_u8; 32];
    for (index, byte) in decoded.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16)
            .expect("test revision digest must be hexadecimal");
    }
    decoded
}

fn revision_from_journal_members(members: &serde_json::Value) -> String {
    let members = members
        .as_array()
        .expect("journal members must be an array");
    let mut input = b"aemeath.storage.dataset.revision.v1\0".to_vec();
    input.extend_from_slice(&(members.len() as u64).to_le_bytes());
    for member in members {
        let member_name = member["名称"]
            .as_str()
            .expect("journal member name must be a string");
        input.extend_from_slice(&(member_name.len() as u64).to_le_bytes());
        input.extend_from_slice(member_name.as_bytes());
        input.extend_from_slice(
            &member["字节数"]
                .as_u64()
                .expect("journal byte count must be an integer")
                .to_le_bytes(),
        );
        input.extend_from_slice(&decode_hex32(
            member["修订摘要"]
                .as_str()
                .expect("journal revision digest must be a string"),
        ));
    }
    hex_sha256(input)
}

fn assert_invalid_journal(root: &Path) {
    let error = runtime()
        .block_on(adapter(root).read_manifest(&key()))
        .expect_err("parseable but semantically invalid journal must fail closed");
    let StorageErrorKind::CorruptTransaction(corruption) = error.kind() else {
        panic!("invalid journal must be typed transaction corruption: {error:?}");
    };
    assert_eq!(
        corruption.reason(),
        storage::CorruptionReason::InvalidJournal
    );
    assert_corrupt_and_quarantined(error, root);
}

fn run_invalid_journal_case(
    case: &str,
    members: String,
    old_revision: String,
    new_revision: String,
) {
    let root = unique_root(case);
    seed(&root, &key(), &old_members());
    let sentinel = dataset_dir(&root, "conversation-1").join("outside");
    std::fs::write(&sentinel, b"must-not-be-touched").expect("write path sentinel");
    std::fs::write(
        dataset_dir(&root, "conversation-1").join("journal.json"),
        journal_json(&members, &old_revision, &new_revision),
    )
    .expect("inject parseable invalid journal");

    assert_invalid_journal(&root);
    assert_eq!(
        std::fs::read(&sentinel).expect("invalid member path must never be resolved or touched"),
        b"must-not-be-touched"
    );
    std::fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn parseable_journal_with_unsafe_member_is_invalid_without_touching_the_path() {
    run_invalid_journal_case(
        "unsafe-member",
        format!(
            r#"[{{"名称":"../../outside","摘要":"{}"}}]"#,
            "11".repeat(32)
        ),
        "00".repeat(32),
        "00".repeat(32),
    );
}

#[test]
fn parseable_journal_with_duplicate_members_is_invalid() {
    let digest = "11".repeat(32);
    run_invalid_journal_case(
        "duplicate-member",
        format!(r#"[{{"名称":"active","摘要":"{digest}"}},{{"名称":"active","摘要":"{digest}"}}]"#),
        "00".repeat(32),
        "00".repeat(32),
    );
}

#[test]
fn parseable_journal_with_bad_digest_is_invalid() {
    run_invalid_journal_case(
        "bad-digest",
        r#"[{"名称":"active","摘要":"not-a-sha256"}]"#.to_string(),
        "00".repeat(32),
        "00".repeat(32),
    );
}

#[test]
fn parseable_journal_with_bad_revision_is_invalid() {
    run_invalid_journal_case(
        "bad-revision",
        format!(r#"[{{"名称":"active","摘要":"{}"}}]"#, "11".repeat(32)),
        "not-a-revision".to_string(),
        "00".repeat(32),
    );
}

#[test]
fn commit_journal_revision_must_match_the_staged_generation() {
    let root = unique_root("commit-revision-contradiction");
    seed(&root, &key(), &old_members());
    assert!(!spawn_fault_child(&root, "after_prepared", true).success());

    rewrite_journal(&root, |journal| {
        let actual = journal["新修订号"]
            .as_str()
            .expect("new revision is a string");
        journal["新修订号"] = serde_json::Value::String(other_hex64(actual, 'a'));
    });

    assert_invalid_journal(&root);
    let outcome = runtime()
        .block_on(adapter(&root).read_consistent(&key(), &[name("active")]))
        .expect("after invalid journal quarantine the untouched primary remains readable");
    let DatasetReadOutcome::Found(read) = outcome else {
        panic!("the healthy old primary must remain available");
    };
    assert_eq!(
        read.members()[0].bytes(),
        b"old-active",
        "a fabricated journal revision must never publish the staged generation"
    );
    std::fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn self_consistent_journal_revision_metadata_must_match_staged_bytes() {
    let root = unique_root("self-consistent-revision-evidence-contradiction");
    seed(&root, &key(), &old_members());
    assert!(!spawn_fault_child(&root, "after_prepared", true).success());

    rewrite_journal(&root, |journal| {
        let members = journal["成员集合"]
            .as_array_mut()
            .expect("journal members must be an array");
        let active = members
            .iter_mut()
            .find(|member| member["名称"] == "active")
            .expect("active member must be journaled");
        assert_eq!(active["字节数"].as_u64(), Some(b"new-active".len() as u64));
        assert_ne!(b"fake-bytes", b"new-active");
        assert_eq!(b"fake-bytes".len(), b"new-active".len());

        // Keep `摘要` untouched so it still authenticates the actual staged bytes, but replace
        // the independent revision evidence with a valid digest for different same-length bytes.
        // Recompute 新修订号 from that forged evidence so the journal remains internally
        // self-consistent; recovery must still anchor revision evidence to the stage itself.
        active["修订摘要"] = serde_json::Value::String(revision_member_digest(b"fake-bytes"));
        journal["新修订号"] =
            serde_json::Value::String(revision_from_journal_members(&journal["成员集合"]));
    });

    assert_invalid_journal(&root);
    let outcome = runtime()
        .block_on(adapter(&root).read_consistent(&key(), &[name("active")]))
        .expect("invalid journal quarantine must leave the old primary readable");
    let DatasetReadOutcome::Found(read) = outcome else {
        panic!("the healthy old primary must remain available");
    };
    assert_eq!(
        read.members()[0].bytes(),
        b"old-active",
        "forged revision metadata must never publish actual stage bytes under a false revision"
    );
    std::fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn malformed_journal_is_typed_corruption_and_evidence_is_quarantined() {
    let root = unique_root("malformed-journal");
    seed(&root, &key(), &old_members());
    std::fs::write(
        dataset_dir(&root, "conversation-1").join("journal.json"),
        b"{not-json",
    )
    .expect("inject malformed journal");
    let error = runtime()
        .block_on(adapter(&root).read_manifest(&key()))
        .expect_err("invalid journal must fail closed");
    let StorageErrorKind::CorruptTransaction(corruption) = error.kind() else {
        panic!("malformed journal must be typed transaction corruption");
    };
    assert_eq!(
        corruption.reason(),
        storage::CorruptionReason::InvalidJournal
    );
    assert_corrupt_and_quarantined(error, &root);
    std::fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn missing_staged_member_after_prepared_is_typed_corruption_and_quarantined() {
    let root = unique_root("missing-stage");
    seed(&root, &key(), &old_members());
    assert!(!spawn_fault_child(&root, "after_prepared", true).success());
    std::fs::remove_file(stage_dir(&root).join("blobs/archive"))
        .expect("remove one fsynced staged member");
    let error = runtime()
        .block_on(adapter(&root).read_manifest(&key()))
        .expect_err("missing committed evidence cannot be treated as rollback");
    assert_corrupt_and_quarantined(error, &root);
    std::fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn staged_member_digest_mismatch_is_typed_corruption_and_quarantined() {
    let root = unique_root("stage-digest");
    seed(&root, &key(), &old_members());
    assert!(!spawn_fault_child(&root, "after_prepared", true).success());
    std::fs::write(stage_dir(&root).join("blobs/archive"), b"tampered-stage")
        .expect("tamper staged member");
    let error = runtime()
        .block_on(adapter(&root).read_consistent(&key(), &[name("active"), name("archive")]))
        .expect_err("staged digest contradiction must fail closed");
    assert_corrupt_and_quarantined(error, &root);
    std::fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn published_member_digest_contradiction_is_typed_corruption_and_quarantined() {
    let root = unique_root("published-digest");
    seed(&root, &key(), &old_members());
    assert!(!spawn_fault_child(&root, "after_member_publish:active", true).success());
    std::fs::write(
        dataset_dir(&root, "conversation-1").join("primary/blobs/active"),
        b"neither-old-nor-new",
    )
    .expect("tamper already-published member");
    let error = runtime()
        .block_on(adapter(&root).read_manifest(&key()))
        .expect_err("published digest contradiction must fail closed");
    assert_corrupt_and_quarantined(error, &root);

    let entries = std::fs::read_dir(dataset_dir(&root, "conversation-1"))
        .expect("dataset directory")
        .map(|entry| {
            entry
                .expect("directory entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect::<Vec<_>>();
    assert!(
        entries
            .iter()
            .any(|name| name.starts_with("primary.corrupt.")),
        "the contradictory primary generation itself is evidence and must be isolated"
    );

    match runtime().block_on(adapter(&root).read_consistent(&key(), &[name("active")])) {
        Err(_) | Ok(DatasetReadOutcome::NotFound) => {}
        Ok(DatasetReadOutcome::Found(read)) => panic!(
            "a second read must remain fail-closed/NotFound, never return quarantined tampered primary bytes: {:?}",
            read.members()
        ),
    }
    std::fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn quarantined_journal_without_marker_keeps_contradictory_primary_fail_closed() {
    let root = unique_root("quarantine-crash-window");
    seed(&root, &key(), &old_members());
    assert!(!spawn_fault_child(&root, "after_member_publish:active", true).success());
    let dataset = dataset_dir(&root, "conversation-1");
    std::fs::write(dataset.join("primary/blobs/active"), b"neither-old-nor-new")
        .expect("make the still-published primary contradict transaction evidence");

    // Model a crash inside quarantine: journal isolation is durable, but primary isolation and
    // corruption.marker creation have not happened yet.
    std::fs::rename(
        dataset.join("journal.json"),
        dataset.join("journal.json.corrupt.crash-window"),
    )
    .expect("move journal to its quarantine name");
    assert!(dataset.join("primary").exists());
    assert!(!dataset.join("corruption.marker").exists());

    for attempt in 1..=2 {
        let error = runtime()
            .block_on(adapter(&root).read_consistent(&key(), &[name("active")]))
            .expect_err("orphaned quarantine evidence must remain a durable fail-closed barrier");
        assert!(
            matches!(error.kind(), StorageErrorKind::CorruptTransaction(_)),
            "reopen attempt {attempt} must report typed persistent corruption: {error:?}"
        );
        assert!(
            dataset.join("primary").exists(),
            "the contradictory primary may remain for quarantine retry but must never be opened"
        );
    }
    std::fs::remove_dir_all(root).expect("cleanup");
}

#[cfg(unix)]
#[test]
fn quarantine_partial_failure_keeps_the_journal_as_a_persistent_fail_closed_barrier() {
    let root = unique_root("quarantine-partial-failure");
    seed(&root, &key(), &old_members());
    assert!(!spawn_fault_child(&root, "after_member_publish:active", true).success());
    let dataset = dataset_dir(&root, "conversation-1");
    let journal = dataset.join("journal.json");
    std::fs::write(dataset.join("primary/blobs/active"), b"neither-old-nor-new")
        .expect("create a published contradiction");

    // Force a *partial* quarantine rather than a blanket permission failure. Reinsert primary
    // after the journal and add enough preceding evidence renames for a watcher to occupy the
    // UUID-correlated primary target once the journal target reveals that UUID. The occupied,
    // non-empty target makes only primary's rename fail on Unix.
    let held_primary = dataset.join("primary-held-for-ordering");
    std::fs::rename(dataset.join("primary"), &held_primary).expect("temporarily hold primary");
    for index in 0..512 {
        std::fs::create_dir(dataset.join(format!(".stage-padding-{index:04}")))
            .expect("create quarantine scheduling evidence");
    }
    std::fs::rename(&held_primary, dataset.join("primary")).expect("reinsert primary last");

    let watched_dataset = dataset.clone();
    let blocker = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if let Ok(entries) = std::fs::read_dir(&watched_dataset) {
                for entry in entries.flatten() {
                    let file_name = entry.file_name().to_string_lossy().into_owned();
                    if let Some(id) = file_name.strip_prefix("journal.json.corrupt.") {
                        let target = watched_dataset.join(format!("primary.corrupt.{id}"));
                        if std::fs::create_dir(&target).is_ok() {
                            std::fs::write(target.join("occupied"), b"force rename failure")
                                .expect("occupy primary quarantine target");
                        }
                        return true;
                    }
                }
            }
            std::thread::yield_now();
        }
        false
    });

    let first = runtime()
        .block_on(adapter(&root).read_manifest(&key()))
        .expect_err("published contradiction must fail closed when primary quarantine fails");
    assert!(
        blocker.join().expect("quarantine blocker thread"),
        "test must observe journal quarantine and inject the correlated primary collision"
    );
    let StorageErrorKind::CorruptTransaction(corruption) = first.kind() else {
        panic!("quarantine failure must remain typed corruption: {first:?}");
    };
    assert_eq!(
        corruption.quarantine_disposition(),
        QuarantineDisposition::QuarantineFailed
    );
    assert!(
        dataset.join("primary").exists(),
        "primary rename was forced to fail"
    );
    assert!(
        journal.exists(),
        "journal is the durable fail-closed barrier and must not disappear before primary is isolated"
    );

    let second = runtime()
        .block_on(adapter(&root).read_consistent(&key(), &[name("active")]))
        .expect_err("a later read must retry recovery, not open the still-published primary");
    assert!(matches!(
        second.kind(),
        StorageErrorKind::CorruptTransaction(_)
    ));
    assert!(
        journal.exists(),
        "repeated failed quarantine must retain the barrier"
    );
    std::fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn corruption_has_priority_over_stale_cas_and_not_found() {
    let root = unique_root("error-priority");
    let stale = {
        let adapter = adapter(&root);
        runtime()
            .block_on(adapter.read_manifest(&key()))
            .expect("empty manifest")
            .revision()
            .clone()
    };
    seed(&root, &key(), &old_members());
    std::fs::write(
        dataset_dir(&root, "conversation-1").join("journal.json"),
        b"invalid",
    )
    .expect("inject malformed journal");

    let adapter = adapter(&root);
    let error = runtime()
        .block_on(adapter.commit_atomic(&key(), &stale, &new_members(), options()))
        .expect_err("recovery runs before CAS comparison");
    assert!(matches!(
        error.kind(),
        StorageErrorKind::CorruptTransaction(_)
    ));

    // The first operation quarantines the evidence. Re-inject malformed evidence
    // to pin the same priority at the read boundary.
    std::fs::write(
        dataset_dir(&root, "conversation-1").join("journal.json"),
        b"invalid",
    )
    .expect("re-inject malformed journal");
    let error = runtime()
        .block_on(adapter.read_consistent(&key(), &[name("absent")]))
        .expect_err("corruption outranks a requested-member NotFound outcome");
    assert!(matches!(
        error.kind(),
        StorageErrorKind::CorruptTransaction(_)
    ));
    std::fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn promote_prepared_journal_must_match_primary_and_previous_evidence() {
    let root = unique_root("promote-evidence-contradiction");
    seed(&root, &key(), &old_members());
    seed(&root, &key(), &new_members());
    assert!(
        !spawn_promote_fault_child(&root, "promote_after_prepared").success(),
        "fault must leave a durable Prepared promote journal"
    );

    rewrite_journal(&root, |journal| {
        let old = journal["旧修订号"]
            .as_str()
            .expect("old revision is a string")
            .to_owned();
        let new = journal["新修订号"]
            .as_str()
            .expect("new revision is a string")
            .to_owned();
        journal["旧修订号"] = serde_json::Value::String(other_hex64(&old, 'c'));
        journal["新修订号"] = serde_json::Value::String(other_hex64(&new, 'd'));
        journal["成员集合"][0]["摘要"] = serde_json::Value::String("e".repeat(64));
    });

    assert_invalid_journal(&root);
    assert_eq!(
        read_pair(&root),
        (b"new-active".to_vec(), b"new-archive".to_vec()),
        "contradictory promote metadata must not exchange the generations"
    );
    assert_eq!(
        read_generation_pair(&root, true),
        (b"old-active".to_vec(), b"old-archive".to_vec()),
        "contradictory promote metadata must leave Previous untouched"
    );
    std::fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn promote_recovery_rejects_tampered_prepared_previous_before_publishing_it() {
    let root = unique_root("promote-prepared-previous-digest");
    seed(&root, &key(), &old_members());
    seed(&root, &key(), &new_members());
    assert!(
        !spawn_promote_fault_child(&root, "promote_after_prepared").success(),
        "fault must leave the promotion durably Prepared"
    );

    let dataset = dataset_dir(&root, "conversation-1");
    let manifest = std::fs::read(dataset.join("previous/manifest.json"))
        .expect("pending Previous manifest must exist");
    let journal =
        std::fs::read(dataset.join("journal.json")).expect("Prepared promotion journal must exist");
    std::fs::write(dataset.join("previous/blobs/archive"), b"tampered-previous")
        .expect("tamper one member of the generation awaiting promotion");
    assert_eq!(
        std::fs::read(dataset.join("previous/manifest.json")).expect("manifest remains readable"),
        manifest,
        "the test must leave the Previous manifest unchanged"
    );
    assert_eq!(
        std::fs::read(dataset.join("journal.json")).expect("journal remains readable"),
        journal,
        "the test must leave the Prepared journal unchanged"
    );

    let error = runtime()
        .block_on(adapter(&root).read_manifest(&key()))
        .expect_err("reopen promotion recovery must authenticate Previous before publishing it");
    let StorageErrorKind::CorruptTransaction(corruption) = error.kind() else {
        panic!("a tampered prepared Previous must be typed transaction corruption: {error:?}");
    };
    assert_eq!(
        corruption.reason(),
        storage::CorruptionReason::DatasetMemberDigestMismatch
    );
    assert_eq!(
        std::fs::read(dataset.join("primary/blobs/archive"))
            .expect("the original Primary must remain published"),
        b"new-archive",
        "recovery must not publish the tampered Previous as Primary"
    );

    std::fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn promote_previous_abort_points_recover_the_complete_swap() {
    for point in [
        "promote_after_prepared",
        "promote_after_primary_to_swap",
        "promote_after_previous_to_primary",
    ] {
        let root = unique_root(point);
        seed(&root, &key(), &old_members());
        seed(&root, &key(), &new_members());

        let status = spawn_promote_fault_child(&root, point);
        assert!(
            !status.success(),
            "private promote fault point {point} must abort"
        );
        assert_eq!(
            read_pair(&root),
            (b"old-active".to_vec(), b"old-archive".to_vec()),
            "reopen after {point} must complete promotion of the old previous generation"
        );
        assert_eq!(
            read_generation_pair(&root, true),
            (b"new-active".to_vec(), b"new-archive".to_vec()),
            "reopen after {point} must retain the complete former primary as Previous"
        );
        assert!(transaction_artifacts(&root, "conversation-1").is_empty());
        std::fs::remove_dir_all(root).expect("cleanup");
    }
}

#[test]
fn promote_rejects_missing_previous_member_before_prepared_without_journal() {
    let root = unique_root("promote-missing-member");
    seed(&root, &key(), &old_members());
    seed(&root, &key(), &new_members());
    std::fs::remove_file(dataset_dir(&root, "conversation-1").join("previous/blobs/archive"))
        .expect("remove previous member");

    let error = runtime()
        .block_on(adapter(&root).promote_previous(&key()))
        .expect_err("an incomplete previous generation must not cross Prepared");
    assert!(
        matches!(error.kind(), StorageErrorKind::CorruptTransaction(_)),
        "frozen semantics prefer typed CorruptTransaction for incomplete Previous: {error:?}"
    );
    assert!(
        !dataset_dir(&root, "conversation-1")
            .join("journal.json")
            .exists(),
        "Previous must be fully validated before writing Prepared"
    );
    std::fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn post_prepared_failure_returns_committed_recovery_pending_receipt() {
    let root = unique_root("pending-receipt");
    let result = root.join("receipt-result");
    seed(&root, &key(), &old_members());
    let status = Command::new(std::env::current_exe().expect("test executable"))
        .arg("--exact")
        .arg("dataset_child_runs_transaction")
        .arg("--nocapture")
        .env(HELPER_MODE, "commit")
        .env(HELPER_ROOT, &root)
        .env(HELPER_RESULT, &result)
        .env(FAULT_POINT, "after_prepared")
        .stdout(Stdio::null())
        .status()
        .expect("receipt child must launch");
    assert!(
        status.success(),
        "post-Prepared ordinary failure must not escape as Err"
    );
    assert_eq!(
        std::fs::read_to_string(result).expect("receipt result"),
        "recovery-pending",
        "Prepared is the logical commit point: return committed revision + RecoveryPending + MemberPublishRecoveryPending"
    );
    // The receipt says committed even if visibility awaits mechanical recovery.
    assert_eq!(
        read_pair(&root),
        (b"new-active".to_vec(), b"new-archive".to_vec())
    );
    std::fs::remove_dir_all(root).expect("cleanup");
}
