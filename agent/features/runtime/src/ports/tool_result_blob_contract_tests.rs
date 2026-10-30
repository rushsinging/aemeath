use std::sync::Arc;

use super::ToolResultBlobPort;
use crate::adapters::tool_result_blob::AtomicBlobToolResultStore;

#[tokio::test]
async fn adapter_maps_ids_to_tool_result_namespace_and_is_write_once_idempotent() {
    let blob = Arc::new(storage::test_support::InMemoryBlobPort::new());
    let store = AtomicBlobToolResultStore::new(blob.clone(), "/root".into());

    let first = store
        .write_once("session-1", "tool-1", b"full output")
        .await
        .unwrap();
    let second = store
        .write_once("session-1", "tool-1", b"full output")
        .await
        .unwrap();

    assert_eq!(first, second);
    assert_eq!(blob.writes().len(), 1);
    let writes = blob.writes();
    let (key, bytes, options) = &writes[0];
    assert_eq!(key.namespace(), storage::StorageNamespaceData::ToolResult);
    assert_eq!(
        key.segments()
            .iter()
            .map(|segment| segment.as_str())
            .collect::<Vec<_>>(),
        vec!["session-1", "tool-1"]
    );
    assert_eq!(bytes, b"full output");
    assert_eq!(
        options.durability(),
        storage::DurabilityData::ProcessCrashSafe
    );
}

#[tokio::test]
async fn adapter_rejects_unsafe_identifiers_before_storage_io() {
    let blob = Arc::new(storage::test_support::InMemoryBlobPort::new());
    let store = AtomicBlobToolResultStore::new(blob.clone(), "/root".into());

    let error = store
        .write_once("../escape", "tool", b"output")
        .await
        .unwrap_err();

    assert!(error.to_string().contains("路径段不安全"));
    assert!(blob.writes().is_empty());
}

#[tokio::test]
async fn adapter_rejects_same_identifier_with_different_content() {
    let blob = Arc::new(storage::test_support::InMemoryBlobPort::new());
    let store = AtomicBlobToolResultStore::new(blob.clone(), "/root".into());
    store.write_once("session", "tool", b"old").await.unwrap();

    let error = store
        .write_once("session", "tool", b"new")
        .await
        .unwrap_err();

    assert!(error.to_string().contains("不同内容"));
    assert_eq!(blob.writes().len(), 1);
    assert_eq!(
        blob.values().first().map(Vec::as_slice),
        Some(b"old".as_slice())
    );
}

#[tokio::test]
async fn adapter_write_failure_returns_no_locator_and_keeps_storage_empty() {
    let blob = Arc::new(storage::test_support::InMemoryBlobPort::new());
    blob.fail_next_write("磁盘不可写");
    let store = AtomicBlobToolResultStore::new(blob.clone(), "/root".into());

    let result = store.write_once("session", "tool", b"full output").await;

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("磁盘不可写"));
    assert!(blob.values().is_empty());
    assert!(blob.writes().is_empty());
}
