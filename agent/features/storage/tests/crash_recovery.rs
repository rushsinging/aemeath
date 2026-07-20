use std::process::{Command, Stdio};
use std::str::FromStr;
use std::time::{Duration, Instant};

use storage::{
    AtomicBlobPort, Durability, FileSystemBlobAdapter, Generation, ReadOutcome, SafePathSegment,
    StorageKey, StorageNamespace, WriteOptions,
};
use uuid::Uuid;

fn root(label: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("aemeath-storage-{label}-{}", Uuid::new_v4()))
}

fn key() -> StorageKey {
    StorageKey::new(
        StorageNamespace::Session,
        vec![SafePathSegment::from_str("session-1").unwrap()],
    )
    .unwrap()
}

#[tokio::test]
async fn replacement_never_moves_primary_before_commit() {
    let root = root("primary-window");
    let adapter = FileSystemBlobAdapter::new(&root).unwrap();
    adapter
        .write_atomic(
            &key(),
            b"old",
            WriteOptions::new(Durability::ProcessCrashSafe),
        )
        .await
        .unwrap();
    adapter
        .write_atomic(
            &key(),
            b"new",
            WriteOptions::new(Durability::ProcessCrashSafe),
        )
        .await
        .unwrap();

    assert_eq!(
        std::fs::read(root.join("session/session-1")).unwrap(),
        b"new"
    );
    assert_eq!(
        std::fs::read(root.join("session/session-1.previous")).unwrap(),
        b"old"
    );
    assert!(std::fs::read_dir(root.join("session"))
        .unwrap()
        .all(|entry| {
            let name = entry.unwrap().file_name().to_string_lossy().into_owned();
            !name.contains("previous.next")
                && !name.contains("journal")
                && !name.starts_with(".stage-")
        }));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn conflicting_prepared_evidence_is_quarantined_with_typed_error() {
    let root = root("corruption");
    let adapter = FileSystemBlobAdapter::new(&root).unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime
        .block_on(adapter.write_atomic(
            &key(),
            b"old",
            WriteOptions::new(Durability::ProcessCrashSafe),
        ))
        .unwrap();
    drop(adapter);
    let status = Command::new(std::env::current_exe().unwrap())
        .arg("--exact")
        .arg("helper_process_aborts_after_replace")
        .env("AEMEATH_STORAGE_CRASH_HELPER", "1")
        .env("AEMEATH_STORAGE_CRASH_ROOT", &root)
        .env("AEMEATH_STORAGE_FAULT_POINT", "after_replace")
        .env("AEMEATH_STORAGE_FAULT_ABORT", "1")
        .status()
        .unwrap();
    assert!(!status.success());
    std::fs::write(root.join("session/session-1"), b"tampered").unwrap();

    let reopened = FileSystemBlobAdapter::new(&root).unwrap();
    let error = runtime
        .block_on(reopened.read(&key(), Generation::Primary))
        .unwrap_err();
    let storage::StorageErrorKind::CorruptTransaction(corruption) = error.kind() else {
        panic!("digest conflict must be typed corruption");
    };
    assert_eq!(
        corruption.reason(),
        storage::CorruptionReason::PrimaryDigestMatchesNeitherGeneration
    );
    assert_eq!(
        corruption.quarantine_disposition(),
        storage::QuarantineDisposition::EvidenceQuarantined
    );
    assert!(std::fs::read_dir(root.join("session"))
        .unwrap()
        .any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains(".corrupt.")
        }));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn helper_process_reports_fault_outcome() {
    if std::env::var_os("AEMEATH_STORAGE_MATRIX_HELPER").is_none() {
        return;
    }
    let root = std::path::PathBuf::from(std::env::var_os("AEMEATH_STORAGE_MATRIX_ROOT").unwrap());
    let result =
        std::path::PathBuf::from(std::env::var_os("AEMEATH_STORAGE_MATRIX_RESULT").unwrap());
    let adapter = FileSystemBlobAdapter::new(&root).unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let outcome = runtime.block_on(adapter.write_atomic(
        &key(),
        b"new",
        WriteOptions::new(Durability::ProcessCrashSafe),
    ));
    let label = match outcome {
        Ok(receipt) if receipt.warning().is_some() => "warning",
        Ok(_) => "committed",
        Err(error) if error.kind() == &storage::StorageErrorKind::UnsupportedDurability => {
            "unsupported"
        }
        Err(_) => "error",
    };
    std::fs::write(result, label).unwrap();
}

#[test]
fn all_protocol_fault_points_preserve_commit_contract() {
    for (point, expected) in [
        ("stage_write", "error"),
        ("file_sync", "error"),
        ("unsupported_durability", "unsupported"),
        ("previous_next", "error"),
        ("prepared_journal", "error"),
        ("directory_sync", "error"),
        ("after_replace", "warning"),
        ("committed_journal", "warning"),
        ("previous_promotion", "warning"),
        ("cleanup", "warning"),
    ] {
        let root = root(point);
        let adapter = FileSystemBlobAdapter::new(&root).unwrap();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime
            .block_on(adapter.write_atomic(
                &key(),
                b"old",
                WriteOptions::new(Durability::ProcessCrashSafe),
            ))
            .unwrap();
        drop(adapter);
        let result = root.join("matrix-result");
        let status = Command::new(std::env::current_exe().unwrap())
            .arg("--exact")
            .arg("helper_process_reports_fault_outcome")
            .env("AEMEATH_STORAGE_MATRIX_HELPER", "1")
            .env("AEMEATH_STORAGE_MATRIX_ROOT", &root)
            .env("AEMEATH_STORAGE_MATRIX_RESULT", &result)
            .env("AEMEATH_STORAGE_FAULT_POINT", point)
            .status()
            .unwrap();
        assert!(status.success(), "fault helper failed at {point}");
        assert_eq!(
            std::fs::read_to_string(result).unwrap(),
            expected,
            "{point}"
        );
        let reopened = FileSystemBlobAdapter::new(&root).unwrap();
        let ReadOutcome::Found(primary) = runtime
            .block_on(reopened.read(&key(), Generation::Primary))
            .unwrap()
        else {
            panic!("{point}: primary must remain readable");
        };
        let expected_bytes: &[u8] = if expected == "warning" {
            b"new"
        } else {
            b"old"
        };
        assert_eq!(primary.bytes(), expected_bytes, "{point}");
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn helper_process_aborts_after_replace() {
    if std::env::var_os("AEMEATH_STORAGE_CRASH_HELPER").is_none() {
        return;
    }
    let root = std::path::PathBuf::from(std::env::var_os("AEMEATH_STORAGE_CRASH_ROOT").unwrap());
    let adapter = FileSystemBlobAdapter::new(&root).unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let _ = runtime.block_on(adapter.write_atomic(
        &key(),
        b"new",
        WriteOptions::new(Durability::ProcessCrashSafe),
    ));
}

#[test]
fn process_abort_after_replace_rolls_forward_on_reopen() {
    let root = root("process-crash");
    let adapter = FileSystemBlobAdapter::new(&root).unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime
        .block_on(adapter.write_atomic(
            &key(),
            b"old",
            WriteOptions::new(Durability::ProcessCrashSafe),
        ))
        .unwrap();
    drop(adapter);

    let status = Command::new(std::env::current_exe().unwrap())
        .arg("--exact")
        .arg("helper_process_aborts_after_replace")
        .env("AEMEATH_STORAGE_CRASH_HELPER", "1")
        .env("AEMEATH_STORAGE_CRASH_ROOT", &root)
        .env("AEMEATH_STORAGE_FAULT_POINT", "after_replace")
        .env("AEMEATH_STORAGE_FAULT_ABORT", "1")
        .status()
        .unwrap();
    assert!(!status.success());

    let reopened = FileSystemBlobAdapter::new(&root).unwrap();
    let ReadOutcome::Found(primary) = runtime
        .block_on(reopened.read(&key(), Generation::Primary))
        .unwrap()
    else {
        panic!("recovery must publish the committed primary");
    };
    assert_eq!(primary.bytes(), b"new");
    let ReadOutcome::Found(previous) = runtime
        .block_on(reopened.read(&key(), Generation::Previous))
        .unwrap()
    else {
        panic!("recovery must promote previous.next");
    };
    assert_eq!(previous.bytes(), b"old");
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn helper_process_reports_post_commit_warning() {
    if std::env::var_os("AEMEATH_STORAGE_WARNING_HELPER").is_none() {
        return;
    }
    let root = std::path::PathBuf::from(std::env::var_os("AEMEATH_STORAGE_WARNING_ROOT").unwrap());
    let result =
        std::path::PathBuf::from(std::env::var_os("AEMEATH_STORAGE_WARNING_RESULT").unwrap());
    let adapter = FileSystemBlobAdapter::new(&root).unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let receipt = runtime
        .block_on(adapter.write_atomic(
            &key(),
            b"new",
            WriteOptions::new(Durability::ProcessCrashSafe),
        ))
        .unwrap();
    assert_eq!(
        receipt.warning(),
        Some(storage::CommitWarning::JournalCleanupPending)
    );
    std::fs::write(result, b"warning").unwrap();
}

#[test]
fn post_commit_fault_returns_committed_warning() {
    let root = root("commit-warning");
    let result = root.join("warning-result");
    let status = Command::new(std::env::current_exe().unwrap())
        .arg("--exact")
        .arg("helper_process_reports_post_commit_warning")
        .env("AEMEATH_STORAGE_WARNING_HELPER", "1")
        .env("AEMEATH_STORAGE_WARNING_ROOT", &root)
        .env("AEMEATH_STORAGE_WARNING_RESULT", &result)
        .env("AEMEATH_STORAGE_FAULT_POINT", "committed_journal")
        .status()
        .unwrap();
    assert!(status.success());
    assert_eq!(std::fs::read(result).unwrap(), b"warning");
    let adapter = FileSystemBlobAdapter::new(&root).unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let ReadOutcome::Found(primary) = runtime
        .block_on(adapter.read(&key(), Generation::Primary))
        .unwrap()
    else {
        panic!("warning receipt must still publish new primary");
    };
    assert_eq!(primary.bytes(), b"new");
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn helper_process_holds_real_os_lock() {
    if std::env::var_os("AEMEATH_STORAGE_LOCK_HELPER").is_none() {
        return;
    }
    let path = std::env::var_os("AEMEATH_STORAGE_LOCK_PATH").unwrap();
    let ready = std::env::var_os("AEMEATH_STORAGE_LOCK_READY").unwrap();
    let release =
        std::path::PathBuf::from(std::env::var_os("AEMEATH_STORAGE_LOCK_RELEASE").unwrap());
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .unwrap();
    fs2::FileExt::lock_exclusive(&file).unwrap();
    std::fs::write(ready, b"ready").unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !release.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(release.exists(), "parent must release the helper lock");
}

#[test]
fn os_lock_serializes_another_process_for_same_key() {
    let root = root("process-lock");
    std::fs::create_dir_all(root.join("session")).unwrap();
    let lock = root.join("session/session-1.lock");
    let ready = root.join("ready");
    let release = root.join("release");
    let reader_started = root.join("reader-started");
    let done = root.join("done");
    let mut child = Command::new(std::env::current_exe().unwrap())
        .arg("--exact")
        .arg("helper_process_holds_real_os_lock")
        .arg("--nocapture")
        .env("AEMEATH_STORAGE_LOCK_HELPER", "1")
        .env("AEMEATH_STORAGE_LOCK_PATH", &lock)
        .env("AEMEATH_STORAGE_LOCK_READY", &ready)
        .env("AEMEATH_STORAGE_LOCK_RELEASE", &release)
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !ready.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(ready.exists(), "helper must acquire the lock");

    let root_for_read = root.clone();
    let done_for_read = done.clone();
    let started_for_read = reader_started.clone();
    let reader = std::thread::spawn(move || {
        let adapter = FileSystemBlobAdapter::new(&root_for_read).unwrap();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        std::fs::write(started_for_read, b"started").unwrap();
        runtime
            .block_on(adapter.read(&key(), Generation::Primary))
            .unwrap();
        std::fs::write(done_for_read, b"done").unwrap();
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    while !reader_started.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(reader_started.exists(), "reader thread must start");
    assert!(
        !done.exists(),
        "same-key read must remain blocked while the helper owns the lock"
    );

    std::fs::write(&release, b"release").unwrap();
    assert!(child.wait().unwrap().success());
    reader.join().unwrap();
    assert!(
        done.exists(),
        "same-key read must finish after lock release"
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn orphan_previous_next_is_cleaned_only_when_matching_primary() {
    let matching_root = root("orphan-match");
    let adapter = FileSystemBlobAdapter::new(&matching_root).unwrap();
    adapter
        .write_atomic(&key(), b"same", WriteOptions::new(Durability::BestEffort))
        .await
        .unwrap();
    std::fs::hard_link(
        matching_root.join("session/session-1"),
        matching_root.join("session/session-1.previous.next"),
    )
    .unwrap();
    adapter.read(&key(), Generation::Primary).await.unwrap();
    assert!(!matching_root
        .join("session/session-1.previous.next")
        .exists());
    std::fs::remove_dir_all(&matching_root).unwrap();

    let mismatch_root = root("orphan-mismatch");
    let adapter = FileSystemBlobAdapter::new(&mismatch_root).unwrap();
    adapter
        .write_atomic(
            &key(),
            b"primary",
            WriteOptions::new(Durability::BestEffort),
        )
        .await
        .unwrap();
    std::fs::write(
        mismatch_root.join("session/session-1.previous.next"),
        b"other",
    )
    .unwrap();
    let error = adapter.read(&key(), Generation::Primary).await.unwrap_err();
    let storage::StorageErrorKind::CorruptTransaction(corruption) = error.kind() else {
        panic!("mismatched orphan must be typed corruption");
    };
    assert_eq!(
        corruption.reason(),
        storage::CorruptionReason::OrphanPreviousDigestMismatch
    );
    std::fs::remove_dir_all(mismatch_root).unwrap();
}

#[tokio::test]
async fn protocol_symlinks_fail_closed_without_touching_outside_file() {
    let root = root("protocol-symlink");
    std::fs::create_dir_all(root.join("session")).unwrap();
    let outside = root.join("outside");
    std::fs::write(&outside, b"safe").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, root.join("session/session-1.lock")).unwrap();
    let adapter = FileSystemBlobAdapter::new(&root).unwrap();
    let error = adapter.read(&key(), Generation::Primary).await.unwrap_err();
    assert_eq!(error.kind(), &storage::StorageErrorKind::InvalidKey);
    assert_eq!(std::fs::read(outside).unwrap(), b"safe");
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn concurrent_writers_serialize_and_leave_no_stage_collision() {
    let root = root("writers");
    let left = FileSystemBlobAdapter::new(&root).unwrap();
    let right = FileSystemBlobAdapter::new(&root).unwrap();
    let key = key();
    let (left_result, right_result) = tokio::join!(
        left.write_atomic(&key, b"left", WriteOptions::new(Durability::BestEffort)),
        right.write_atomic(&key, b"right", WriteOptions::new(Durability::BestEffort)),
    );
    left_result.unwrap();
    right_result.unwrap();
    let ReadOutcome::Found(primary) = left.read(&key, Generation::Primary).await.unwrap() else {
        panic!("serialized writers must leave a primary");
    };
    let ReadOutcome::Found(previous) = left.read(&key, Generation::Previous).await.unwrap() else {
        panic!("serialized writers must retain the other complete generation");
    };
    assert_ne!(primary.bytes(), previous.bytes());
    assert!(std::fs::read_dir(root.join("session"))
        .unwrap()
        .all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".stage-")
        }));
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn promote_idempotency_survives_adapter_reopen() {
    let root = root("promote-reopen");
    let key = key();
    {
        let adapter = FileSystemBlobAdapter::new(&root).unwrap();
        adapter
            .write_atomic(&key, b"old", WriteOptions::new(Durability::BestEffort))
            .await
            .unwrap();
        adapter
            .write_atomic(&key, b"new", WriteOptions::new(Durability::BestEffort))
            .await
            .unwrap();
        assert!(matches!(
            adapter.promote_previous(&key).await.unwrap(),
            storage::PromoteOutcome::Promoted(_)
        ));
    }

    let reopened = FileSystemBlobAdapter::new(&root).unwrap();
    assert_eq!(
        reopened.promote_previous(&key).await.unwrap(),
        storage::PromoteOutcome::AlreadyPromoted
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn reopen_observes_a_settled_primary() {
    let root = root("reopen");
    {
        let adapter = FileSystemBlobAdapter::new(&root).unwrap();
        adapter
            .write_atomic(
                &key(),
                b"value",
                WriteOptions::new(Durability::ProcessCrashSafe),
            )
            .await
            .unwrap();
    }
    let reopened = FileSystemBlobAdapter::new(&root).unwrap();
    let ReadOutcome::Found(value) = reopened.read(&key(), Generation::Primary).await.unwrap()
    else {
        panic!("primary must survive reopen");
    };
    assert_eq!(value.bytes(), b"value");
    std::fs::remove_dir_all(root).unwrap();
}
