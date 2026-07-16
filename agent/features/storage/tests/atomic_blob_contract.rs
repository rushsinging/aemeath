#[cfg(unix)]
use std::os::unix::fs::symlink;
use std::str::FromStr;
use storage::{
    AtomicBlobPort, Durability, FileSystemBlobAdapter, Generation, ReadOutcome, SafePathSegment,
    StorageErrorKind, StorageKey, StorageNamespace, WriteOptions,
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

    assert_eq!(error.kind(), StorageErrorKind::InvalidKey);
    assert_eq!(std::fs::read(outside_file).unwrap(), b"outside");

    std::fs::remove_dir_all(root).unwrap();
    std::fs::remove_dir_all(outside).unwrap();
}
