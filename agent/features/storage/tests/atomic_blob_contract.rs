#[cfg(unix)]
use std::os::unix::fs::symlink;
use std::str::FromStr;
use storage::{
    AtomicBlobPort, DeleteOptionsData, DurabilityData, GenerationData, PromoteOutcomeData,
    QuarantineOutcomeData, QuarantineReason, ReadOutcomeData, SafePathSegmentData,
    StorageErrorKind, StorageKeyData, StorageNamespaceData, TransactionScopeData, WriteOptionsData,
};
use uuid::Uuid;

fn unique_root(case: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("aemeath-storage-{case}-{}", Uuid::new_v4()))
}

fn key() -> StorageKeyData {
    StorageKeyData::new(
        StorageNamespaceData::Session,
        vec![SafePathSegmentData::from_str("session-1").expect("valid segment")],
    )
    .expect("valid key")
}

async fn assert_atomic_blob_contract(port: &dyn AtomicBlobPort) {
    let key = key();
    assert_eq!(
        port.read(&key, GenerationData::Primary).await.unwrap(),
        ReadOutcomeData::NotFound
    );

    let receipt = port
        .write_atomic(
            &key,
            b"first",
            WriteOptionsData::new(DurabilityData::BestEffort),
        )
        .await
        .expect("write must commit");
    assert_eq!(receipt.warning(), None);

    let ReadOutcomeData::Found(read) = port
        .read(&key, GenerationData::Primary)
        .await
        .expect("read must succeed")
    else {
        panic!("committed primary must exist");
    };
    assert_eq!(read.generation(), GenerationData::Primary);
    assert_eq!(read.bytes(), b"first");

    assert_eq!(
        port.read(&key, GenerationData::Previous).await.unwrap(),
        ReadOutcomeData::NotFound,
        "read must never fall back across generations"
    );

    port.write_atomic(
        &key,
        b"second",
        WriteOptionsData::new(DurabilityData::BestEffort),
    )
    .await
    .expect("replacement must commit");
    assert_generation(port, &key, GenerationData::Primary, b"second").await;
    assert_generation(port, &key, GenerationData::Previous, b"first").await;

    let PromoteOutcomeData::Promoted(receipt) = port
        .promote_previous(&key)
        .await
        .expect("promote must succeed")
    else {
        panic!("existing previous must be promoted");
    };
    assert_eq!(receipt.warning(), None);
    assert_generation(port, &key, GenerationData::Primary, b"first").await;
    assert_eq!(
        port.promote_previous(&key).await.unwrap(),
        PromoteOutcomeData::AlreadyPromoted
    );
    assert_generation(port, &key, GenerationData::Primary, b"first").await;

    let outcome = port
        .quarantine(
            &key,
            GenerationData::Primary,
            TransactionScopeData::Blob,
            QuarantineReason::DecoderRejected,
        )
        .await
        .expect("quarantine must succeed");
    assert!(matches!(outcome, QuarantineOutcomeData::Moved(_)));
    assert_eq!(outcome.generation(), GenerationData::Primary);
    assert_eq!(outcome.scope(), TransactionScopeData::Blob);
    assert_eq!(outcome.reason(), QuarantineReason::DecoderRejected);
    assert_eq!(
        port.read(&key, GenerationData::Primary).await.unwrap(),
        ReadOutcomeData::NotFound
    );

    let absent = port
        .quarantine(
            &key,
            GenerationData::Primary,
            TransactionScopeData::Blob,
            QuarantineReason::DecoderRejected,
        )
        .await
        .unwrap();
    assert!(matches!(
        absent,
        QuarantineOutcomeData::AlreadyAbsent { .. }
    ));

    let deleted = port
        .delete_all_generations(&key, DeleteOptionsData::default())
        .await
        .expect("delete-all must succeed");
    assert!(!deleted.deleted_primary());
    assert!(!deleted.deleted_previous());
    assert!(deleted.deleted_quarantine());
    let repeated = port
        .delete_all_generations(&key, DeleteOptionsData::default())
        .await
        .unwrap();
    assert!(!repeated.deleted_primary());
    assert!(!repeated.deleted_previous());
    assert!(!repeated.deleted_quarantine());
}

async fn assert_generation(
    port: &dyn AtomicBlobPort,
    key: &StorageKeyData,
    generation: GenerationData,
    expected: &[u8],
) {
    let ReadOutcomeData::Found(read) = port.read(key, generation).await.unwrap() else {
        panic!("requested generation must exist: {generation:?}");
    };
    assert_eq!(read.generation(), generation);
    assert_eq!(read.bytes(), expected);
}

#[cfg(unix)]
#[tokio::test]
async fn list_primary_hides_protocol_files_and_rejects_symlink_entries() {
    let root = unique_root("list-primary-protocol");
    let outside = unique_root("list-primary-outside");
    std::fs::create_dir_all(root.join("session")).expect("create namespace directory");
    std::fs::create_dir_all(&outside).expect("create outside directory");
    std::fs::write(root.join("session/visible"), b"visible").expect("write primary");
    for name in [
        "visible.previous",
        "visible.previous.next",
        "visible.journal",
        "visible.lock",
        "visible.promoted",
        "visible.quarantine.evidence",
        ".stage-nonce",
        ".journal-nonce",
    ] {
        std::fs::write(root.join("session").join(name), b"protocol").expect("write protocol file");
    }
    std::fs::create_dir_all(root.join("session/nested")).expect("create nested directory");
    let outside_file = outside.join("escape");
    std::fs::write(&outside_file, b"outside").expect("write outside file");
    symlink(&outside_file, root.join("session/unsafe")).expect("create symlink");

    let adapter = storage::wire_file_system_blob(&root).expect("adapter root should initialize");
    let error = adapter
        .list_primary(StorageNamespaceData::Session)
        .await
        .expect_err("symlink entry must fail closed");
    assert_eq!(error.kind(), &StorageErrorKind::InvalidKey);
    std::fs::remove_file(root.join("session/unsafe")).expect("remove symlink");

    let entries = adapter
        .list_primary(StorageNamespaceData::Session)
        .await
        .expect("list primary after removing symlink");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].key().segments()[0].as_str(), "visible");

    std::fs::remove_dir_all(root).expect("remove test root");
    std::fs::remove_dir_all(outside).expect("remove outside root");
}

#[tokio::test]
async fn list_primary_returns_only_top_level_primary_entries_for_namespace() {
    let root = unique_root("list-primary");
    let adapter = storage::wire_file_system_blob(&root).expect("adapter root should initialize");
    let first = StorageKeyData::new(
        StorageNamespaceData::Session,
        vec![SafePathSegmentData::from_str("first").expect("valid entry")],
    )
    .expect("valid first key");
    let second = StorageKeyData::new(
        StorageNamespaceData::Session,
        vec![SafePathSegmentData::from_str("second").expect("valid entry")],
    )
    .expect("valid second key");

    adapter
        .write_atomic(
            &first,
            b"first",
            WriteOptionsData::new(DurabilityData::BestEffort),
        )
        .await
        .expect("write first");
    adapter
        .write_atomic(
            &first,
            b"first-next",
            WriteOptionsData::new(DurabilityData::BestEffort),
        )
        .await
        .expect("replace first");
    adapter
        .write_atomic(
            &second,
            b"second",
            WriteOptionsData::new(DurabilityData::BestEffort),
        )
        .await
        .expect("write second");

    let entries = adapter
        .list_primary(StorageNamespaceData::Session)
        .await
        .expect("list primary");
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].key(), &first);
    assert_eq!(entries[0].generation(), GenerationData::Primary);
    assert_eq!(entries[0].size_bytes(), b"first-next".len());
    assert_eq!(entries[1].key(), &second);
    assert_eq!(entries[1].generation(), GenerationData::Primary);
    assert_eq!(entries[1].size_bytes(), b"second".len());

    std::fs::remove_dir_all(root).expect("remove test root");
}

#[tokio::test]
async fn filesystem_adapter_satisfies_atomic_blob_contract() {
    let root = unique_root("contract");
    let adapter = storage::wire_file_system_blob(&root).expect("adapter root should initialize");

    assert_atomic_blob_contract(&*adapter).await;

    std::fs::remove_dir_all(root).expect("temporary root should be removable");
}

#[tokio::test]
async fn filesystem_adapter_replaces_primary_with_complete_value() {
    let root = unique_root("replace");
    let adapter = storage::wire_file_system_blob(&root).expect("adapter root should initialize");
    let key = key();

    adapter
        .write_atomic(
            &key,
            b"old",
            WriteOptionsData::new(DurabilityData::BestEffort),
        )
        .await
        .unwrap();
    adapter
        .write_atomic(
            &key,
            b"new",
            WriteOptionsData::new(DurabilityData::BestEffort),
        )
        .await
        .unwrap();

    let ReadOutcomeData::Found(read) = adapter.read(&key, GenerationData::Primary).await.unwrap()
    else {
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
    let adapter = storage::wire_file_system_blob(&root).expect("adapter root should initialize");
    let key = key();
    adapter
        .write_atomic(
            &key,
            b"old",
            WriteOptionsData::new(DurabilityData::BestEffort),
        )
        .await
        .unwrap();
    adapter
        .write_atomic(
            &key,
            b"new",
            WriteOptionsData::new(DurabilityData::BestEffort),
        )
        .await
        .unwrap();

    let outcome = adapter
        .quarantine(
            &key,
            GenerationData::Previous,
            TransactionScopeData::Blob,
            QuarantineReason::DecoderRejected,
        )
        .await
        .unwrap();

    let QuarantineOutcomeData::Moved(receipt) = outcome else {
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
    let adapter = storage::wire_file_system_blob(&root).expect("adapter root should initialize");

    let error = adapter
        .write_atomic(
            &key(),
            b"new",
            WriteOptionsData::new(DurabilityData::BestEffort),
        )
        .await
        .expect_err("symlink target must fail closed");

    assert_eq!(error.kind(), &StorageErrorKind::InvalidKey);
    assert_eq!(std::fs::read(outside_file).unwrap(), b"outside");

    std::fs::remove_dir_all(root).unwrap();
    std::fs::remove_dir_all(outside).unwrap();
}
