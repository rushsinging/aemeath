#[cfg(unix)]
use std::os::unix::fs::symlink;
use std::str::FromStr;
use storage::{
    AtomicBlobPort, DeleteOptions, Durability, FileSystemBlobAdapter, Generation, PromoteOutcome,
    QuarantineOutcome, QuarantineReason, ReadOutcome, SafePathSegment, StorageErrorKind,
    StorageKey, StorageNamespace, TransactionScope, WriteOptions,
};
use uuid::Uuid;

fn unique_root(case: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("aemeath-storage-{case}-{}", Uuid::new_v4()))
}

fn key() -> StorageKey {
    StorageKey::new(
        StorageNamespace::Session,
        vec![SafePathSegment::from_str("session-1").expect("valid segment")],
    )
    .expect("valid key")
}

async fn assert_atomic_blob_contract(port: &dyn AtomicBlobPort) {
    let key = key();
    assert_eq!(
        port.read(&key, Generation::Primary).await.unwrap(),
        ReadOutcome::NotFound
    );

    let receipt = port
        .write_atomic(&key, b"first", WriteOptions::new(Durability::BestEffort))
        .await
        .expect("write must commit");
    assert_eq!(receipt.warning(), None);

    let ReadOutcome::Found(read) = port
        .read(&key, Generation::Primary)
        .await
        .expect("read must succeed")
    else {
        panic!("committed primary must exist");
    };
    assert_eq!(read.generation(), Generation::Primary);
    assert_eq!(read.bytes(), b"first");

    assert_eq!(
        port.read(&key, Generation::Previous).await.unwrap(),
        ReadOutcome::NotFound,
        "read must never fall back across generations"
    );

    port.write_atomic(&key, b"second", WriteOptions::new(Durability::BestEffort))
        .await
        .expect("replacement must commit");
    assert_generation(port, &key, Generation::Primary, b"second").await;
    assert_generation(port, &key, Generation::Previous, b"first").await;

    let PromoteOutcome::Promoted(receipt) = port
        .promote_previous(&key)
        .await
        .expect("promote must succeed")
    else {
        panic!("existing previous must be promoted");
    };
    assert_eq!(receipt.warning(), None);
    assert_generation(port, &key, Generation::Primary, b"first").await;
    assert_eq!(
        port.promote_previous(&key).await.unwrap(),
        PromoteOutcome::AlreadyPromoted
    );
    assert_generation(port, &key, Generation::Primary, b"first").await;

    let outcome = port
        .quarantine(
            &key,
            Generation::Primary,
            TransactionScope::Blob,
            QuarantineReason::DecoderRejected,
        )
        .await
        .expect("quarantine must succeed");
    assert!(matches!(outcome, QuarantineOutcome::Moved(_)));
    assert_eq!(outcome.generation(), Generation::Primary);
    assert_eq!(outcome.scope(), TransactionScope::Blob);
    assert_eq!(outcome.reason(), QuarantineReason::DecoderRejected);
    assert_eq!(
        port.read(&key, Generation::Primary).await.unwrap(),
        ReadOutcome::NotFound
    );

    let absent = port
        .quarantine(
            &key,
            Generation::Primary,
            TransactionScope::Blob,
            QuarantineReason::DecoderRejected,
        )
        .await
        .unwrap();
    assert!(matches!(absent, QuarantineOutcome::AlreadyAbsent { .. }));

    let deleted = port
        .delete_all_generations(&key, DeleteOptions::default())
        .await
        .expect("delete-all must succeed");
    assert!(!deleted.deleted_primary());
    assert!(!deleted.deleted_previous());
    assert!(deleted.deleted_quarantine());
    let repeated = port
        .delete_all_generations(&key, DeleteOptions::default())
        .await
        .unwrap();
    assert!(!repeated.deleted_primary());
    assert!(!repeated.deleted_previous());
    assert!(!repeated.deleted_quarantine());
}

async fn assert_generation(
    port: &dyn AtomicBlobPort,
    key: &StorageKey,
    generation: Generation,
    expected: &[u8],
) {
    let ReadOutcome::Found(read) = port.read(key, generation).await.unwrap() else {
        panic!("requested generation must exist: {generation:?}");
    };
    assert_eq!(read.generation(), generation);
    assert_eq!(read.bytes(), expected);
}

#[tokio::test]
async fn filesystem_adapter_satisfies_atomic_blob_contract() {
    let root = unique_root("contract");
    let adapter = FileSystemBlobAdapter::new(&root).expect("adapter root should initialize");

    assert_atomic_blob_contract(&adapter).await;

    std::fs::remove_dir_all(root).expect("temporary root should be removable");
}

#[tokio::test]
async fn filesystem_adapter_replaces_primary_with_complete_value() {
    let root = unique_root("replace");
    let adapter = FileSystemBlobAdapter::new(&root).expect("adapter root should initialize");
    let key = key();

    adapter
        .write_atomic(&key, b"old", WriteOptions::new(Durability::BestEffort))
        .await
        .unwrap();
    adapter
        .write_atomic(&key, b"new", WriteOptions::new(Durability::BestEffort))
        .await
        .unwrap();

    let ReadOutcome::Found(read) = adapter.read(&key, Generation::Primary).await.unwrap() else {
        panic!("replaced primary must exist");
    };
    assert_eq!(read.bytes(), b"new");
    assert_eq!(
        std::fs::read(root.join("session/session-1.previous")).unwrap(),
        b"old",
        "replacement must retain the complete old primary"
    );
    assert!(
        std::fs::read_dir(root.join("session"))
            .unwrap()
            .all(|entry| !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".stage-")),
        "successful replace must not leave stage files"
    );

    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn filesystem_adapter_quarantine_moves_only_requested_generation() {
    let root = unique_root("quarantine-layout");
    let adapter = FileSystemBlobAdapter::new(&root).expect("adapter root should initialize");
    let key = key();
    adapter
        .write_atomic(&key, b"old", WriteOptions::new(Durability::BestEffort))
        .await
        .unwrap();
    adapter
        .write_atomic(&key, b"new", WriteOptions::new(Durability::BestEffort))
        .await
        .unwrap();

    let outcome = adapter
        .quarantine(
            &key,
            Generation::Previous,
            TransactionScope::Blob,
            QuarantineReason::DecoderRejected,
        )
        .await
        .unwrap();

    let QuarantineOutcome::Moved(receipt) = outcome else {
        panic!("existing previous must move to quarantine");
    };
    let quarantine_path = root
        .join("session")
        .join(format!("session-1.quarantine.{}", receipt.id()));
    assert_eq!(std::fs::read(quarantine_path).unwrap(), b"old");
    assert_eq!(
        std::fs::read(root.join("session/session-1")).unwrap(),
        b"new",
        "quarantining previous must not touch primary"
    );
    assert!(!root.join("session/session-1.previous").exists());

    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn filesystem_adapter_rejects_symlink_target_without_touching_outside_file() {
    let root = unique_root("symlink");
    let outside = unique_root("outside");
    std::fs::create_dir_all(root.join("session")).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    let outside_file = outside.join("target");
    std::fs::write(&outside_file, b"outside").unwrap();
    symlink(&outside_file, root.join("session/session-1")).unwrap();
    let adapter = FileSystemBlobAdapter::new(&root).expect("adapter root should initialize");

    let error = adapter
        .write_atomic(&key(), b"new", WriteOptions::new(Durability::BestEffort))
        .await
        .expect_err("symlink target must fail closed");

    assert_eq!(error.kind(), &StorageErrorKind::InvalidKey);
    assert_eq!(std::fs::read(outside_file).unwrap(), b"outside");

    std::fs::remove_dir_all(root).unwrap();
    std::fs::remove_dir_all(outside).unwrap();
}
