use memory::{
    AtomicDatasetReflectionHistoryStore, MemoryError, MemoryStorageErrorKind, ProjectMemoryKey,
    ReflectionErrorCategory, ReflectionHistoryQuery, ReflectionHistoryStore, ReflectionRecord,
    ReflectionTrigger,
};
use std::{str::FromStr, sync::Arc};
use storage::api as storage_api;

fn unique_root(case: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "aemeath-reflection-history-{case}-{}",
        uuid::Uuid::new_v4()
    ))
}

fn project_key() -> ProjectMemoryKey {
    ProjectMemoryKey::derive("/reflection/history/contract", None).unwrap()
}

fn storage(root: &std::path::Path) -> Arc<dyn storage_api::AtomicDatasetPort> {
    Arc::new(storage::FileSystemDatasetAdapter::new(root).unwrap())
}

fn store(root: &std::path::Path) -> AtomicDatasetReflectionHistoryStore {
    AtomicDatasetReflectionHistoryStore::new(storage(root), project_key())
}

fn record(id: &str, timestamp: u64) -> ReflectionRecord {
    ReflectionRecord::failed(
        id,
        timestamp,
        ReflectionTrigger::Manual,
        ReflectionErrorCategory::TimedOut,
        25,
    )
}

#[tokio::test]
async fn reflection_history_upsert_replaces_stable_id_without_duplication() {
    let root = unique_root("upsert");
    let history = store(&root);
    let running = memory::ReflectionRecord::running("stable", 40, ReflectionTrigger::Manual);
    history.append(&running).await.unwrap();
    let terminal = record("stable", 40);
    history.upsert(&terminal).await.unwrap();

    assert_eq!(history.list(10).await.unwrap(), vec![terminal]);
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn reflection_history_append_and_list_round_trip() {
    let root = unique_root("append-list");
    let history = store(&root);
    let first = record("first", 10);
    let second = record("second", 20);

    history.append(&first).await.unwrap();
    history.append(&second).await.unwrap();

    assert_eq!(history.list(10).await.unwrap(), vec![second, first]);
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn reflection_history_reopen_keeps_records() {
    let root = unique_root("reopen");
    let expected = record("durable", 30);
    store(&root).append(&expected).await.unwrap();

    let reopened = store(&root);
    assert_eq!(reopened.list(10).await.unwrap(), vec![expected]);
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn reflection_history_limit_returns_newest_records_only() {
    let root = unique_root("limit");
    let history = store(&root);
    for (id, timestamp) in [("one", 1), ("two", 2), ("three", 3)] {
        history.append(&record(id, timestamp)).await.unwrap();
    }

    assert_eq!(
        history
            .list(2)
            .await
            .unwrap()
            .iter()
            .map(|record| record.id.as_str())
            .collect::<Vec<_>>(),
        vec!["three", "two"]
    );
    assert!(history.list(0).await.unwrap().is_empty());
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn reflection_history_corruption_fails_closed() {
    let root = unique_root("corruption");
    let storage = storage(&root);
    let project = project_key();
    let key = storage_api::DatasetKey::new(
        storage_api::StorageNamespace::Memory,
        vec![
            storage_api::SafePathSegment::from_str(project.as_str()).unwrap(),
            storage_api::SafePathSegment::from_str("reflection-history").unwrap(),
        ],
    )
    .unwrap();
    let manifest = storage.read_manifest(&key).await.unwrap();
    storage
        .commit_atomic(
            &key,
            manifest.revision(),
            &[storage_api::DatasetMember::new(
                storage_api::SafePathSegment::from_str("records").unwrap(),
                br#"{"raw_prompt":"must not be accepted"}"#.to_vec(),
            )],
            storage_api::WriteOptions::new(storage_api::Durability::ProcessCrashSafe),
        )
        .await
        .unwrap();

    let error = AtomicDatasetReflectionHistoryStore::new(storage, project)
        .list(10)
        .await
        .unwrap_err();
    assert_eq!(
        error,
        MemoryError::Storage {
            kind: MemoryStorageErrorKind::Serialization
        }
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn reflection_history_adapter_is_memory_owned_port() {
    fn assert_store<T: ReflectionHistoryStore + ReflectionHistoryQuery>() {}
    assert_store::<AtomicDatasetReflectionHistoryStore>();
}
