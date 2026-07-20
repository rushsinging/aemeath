use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use runtime::adapters::tool_result_blob::AtomicBlobToolResultStore;
use runtime::ports::ToolResultBlobPort;
use storage::api::{
    AtomicBlobPort, BlobRead, DeleteOptions, DeleteOutcome, Generation, PromoteOutcome,
    QuarantineOutcome, QuarantineReason, ReadOutcome, StorageError, StorageKey, TransactionScope,
    WriteOptions, WriteReceipt,
};

#[derive(Default)]
struct FakeBlobPort {
    values: Mutex<HashMap<StorageKey, Vec<u8>>>,
    writes: Mutex<Vec<(StorageKey, Vec<u8>, WriteOptions)>>,
}

#[async_trait]
impl AtomicBlobPort for FakeBlobPort {
    async fn read(
        &self,
        key: &StorageKey,
        generation: Generation,
    ) -> Result<ReadOutcome, StorageError> {
        if generation == Generation::Previous {
            return Ok(ReadOutcome::NotFound);
        }
        Ok(match self.values.lock().unwrap().get(key) {
            Some(bytes) => ReadOutcome::Found(BlobRead::new(generation, bytes.clone())),
            None => ReadOutcome::NotFound,
        })
    }

    async fn write_atomic(
        &self,
        key: &StorageKey,
        bytes: &[u8],
        options: WriteOptions,
    ) -> Result<WriteReceipt, StorageError> {
        self.values
            .lock()
            .unwrap()
            .insert(key.clone(), bytes.to_vec());
        self.writes
            .lock()
            .unwrap()
            .push((key.clone(), bytes.to_vec(), options));
        Ok(WriteReceipt::committed(None))
    }

    async fn promote_previous(&self, _key: &StorageKey) -> Result<PromoteOutcome, StorageError> {
        Ok(PromoteOutcome::NotFound)
    }

    async fn quarantine(
        &self,
        _key: &StorageKey,
        _generation: Generation,
        _scope: TransactionScope,
        _reason: QuarantineReason,
    ) -> Result<QuarantineOutcome, StorageError> {
        Ok(QuarantineOutcome::already_absent(
            Generation::Primary,
            TransactionScope::Blob,
            QuarantineReason::DecoderRejected,
        ))
    }

    async fn delete_all_generations(
        &self,
        _key: &StorageKey,
        _options: DeleteOptions,
    ) -> Result<DeleteOutcome, StorageError> {
        Ok(DeleteOutcome::new(false, false, false))
    }

    async fn list_primary(
        &self,
        _namespace: storage::api::StorageNamespace,
    ) -> Result<Vec<storage::api::StorageEntry>, StorageError> {
        Ok(Vec::new())
    }
}

#[tokio::test]
async fn adapter_maps_ids_to_tool_result_namespace_and_is_write_once_idempotent() {
    let blob = Arc::new(FakeBlobPort::default());
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
    assert_eq!(blob.writes.lock().unwrap().len(), 1);
    let writes = blob.writes.lock().unwrap();
    let (key, bytes, options) = &writes[0];
    assert_eq!(key.namespace(), storage::api::StorageNamespace::ToolResult);
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
        storage::api::Durability::ProcessCrashSafe
    );
}

#[tokio::test]
async fn adapter_rejects_unsafe_identifiers_before_storage_io() {
    let blob = Arc::new(FakeBlobPort::default());
    let store = AtomicBlobToolResultStore::new(blob.clone(), "/root".into());

    let error = store
        .write_once("../escape", "tool", b"output")
        .await
        .unwrap_err();

    assert!(error.to_string().contains("路径段不安全"));
    assert!(blob.writes.lock().unwrap().is_empty());
}

#[tokio::test]
async fn adapter_rejects_same_identifier_with_different_content() {
    let blob = Arc::new(FakeBlobPort::default());
    let store = AtomicBlobToolResultStore::new(blob.clone(), "/root".into());
    store.write_once("session", "tool", b"old").await.unwrap();

    let error = store
        .write_once("session", "tool", b"new")
        .await
        .unwrap_err();

    assert!(error.to_string().contains("不同内容"));
    assert_eq!(blob.writes.lock().unwrap().len(), 1);
}
