use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use context::adapters::AtomicBlobSessionStore;
use context::ports::{SessionGeneration, SessionSnapshotStore};
use storage::api::{
    AtomicBlobPort, DeleteOptions, DeleteOutcome, Generation, PromoteOutcome, QuarantineOutcome,
    QuarantineReason, ReadOutcome, StorageError, StorageKey, TransactionScope, WriteOptions,
    WriteReceipt,
};

struct RecordingBlob {
    reads: Mutex<Vec<Generation>>,
    quarantines: Mutex<Vec<(Generation, TransactionScope, QuarantineReason)>>,
    writes: Mutex<Vec<(Vec<u8>, WriteOptions)>>,
    promote_outcome: Mutex<PromoteOutcome>,
    delete_options: Mutex<Vec<DeleteOptions>>,
}

impl Default for RecordingBlob {
    fn default() -> Self {
        Self {
            reads: Mutex::new(Vec::new()),
            quarantines: Mutex::new(Vec::new()),
            writes: Mutex::new(Vec::new()),
            promote_outcome: Mutex::new(PromoteOutcome::AlreadyPromoted),
            delete_options: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl AtomicBlobPort for RecordingBlob {
    async fn read(
        &self,
        _key: &StorageKey,
        generation: Generation,
    ) -> Result<ReadOutcome, StorageError> {
        self.reads.lock().unwrap().push(generation);
        Ok(ReadOutcome::NotFound)
    }
    async fn write_atomic(
        &self,
        _key: &StorageKey,
        bytes: &[u8],
        options: WriteOptions,
    ) -> Result<WriteReceipt, StorageError> {
        self.writes.lock().unwrap().push((bytes.to_vec(), options));
        Ok(WriteReceipt::committed(None))
    }
    async fn promote_previous(&self, _key: &StorageKey) -> Result<PromoteOutcome, StorageError> {
        Ok(*self.promote_outcome.lock().unwrap())
    }
    async fn quarantine(
        &self,
        _key: &StorageKey,
        generation: Generation,
        scope: TransactionScope,
        reason: QuarantineReason,
    ) -> Result<QuarantineOutcome, StorageError> {
        self.quarantines
            .lock()
            .unwrap()
            .push((generation, scope, reason));
        Ok(QuarantineOutcome::already_absent(generation, scope, reason))
    }
    async fn delete_all_generations(
        &self,
        _key: &StorageKey,
        options: DeleteOptions,
    ) -> Result<DeleteOutcome, StorageError> {
        self.delete_options.lock().unwrap().push(options);
        Ok(DeleteOutcome::new(false, false, false))
    }
}

#[tokio::test]
async fn adapter_maps_context_operations_to_session_atomic_blob_contract() {
    let blob = Arc::new(RecordingBlob::default());
    let store = AtomicBlobSessionStore::new(blob.clone(), "session-1").unwrap();
    assert!(store
        .read(SessionGeneration::Primary)
        .await
        .unwrap()
        .is_none());
    store.write(b"canonical").await.unwrap();
    store.quarantine(SessionGeneration::Previous).await.unwrap();
    assert_eq!(*blob.reads.lock().unwrap(), vec![Generation::Primary]);
    let writes = blob.writes.lock().unwrap();
    assert_eq!(writes.len(), 1);
    assert_eq!(writes[0].0, b"canonical");
    assert_eq!(
        writes[0].1.durability(),
        storage::api::Durability::ProcessCrashSafe
    );
    assert_eq!(
        *blob.quarantines.lock().unwrap(),
        vec![(
            Generation::Previous,
            TransactionScope::Blob,
            QuarantineReason::DecoderRejected
        )]
    );
}

#[tokio::test]
async fn adapter_maps_promote_and_delete_to_atomic_blob_contract() {
    let blob = Arc::new(RecordingBlob::default());
    let store = AtomicBlobSessionStore::new(blob.clone(), "session-1").unwrap();

    store.promote_previous().await.unwrap();
    store.delete_all().await.unwrap();

    let options = blob.delete_options.lock().unwrap();
    assert_eq!(options.len(), 1);
    assert!(options[0].include_quarantine());
}

#[tokio::test]
async fn adapter_reports_missing_previous_generation() {
    let blob = Arc::new(RecordingBlob::default());
    *blob.promote_outcome.lock().unwrap() = PromoteOutcome::NotFound;
    let store = AtomicBlobSessionStore::new(blob, "session-1").unwrap();

    let error = store.promote_previous().await.unwrap_err();
    assert!(error.to_string().contains("not found"));
}

#[test]
fn adapter_rejects_unsafe_session_id_before_touching_storage() {
    let blob = Arc::new(RecordingBlob::default());
    assert!(AtomicBlobSessionStore::new(blob, "../escape").is_err());
}
