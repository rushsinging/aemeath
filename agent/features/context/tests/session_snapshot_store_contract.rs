use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use context::adapters::AtomicBlobSessionStore;
use context::ports::{SessionGeneration, SessionSnapshotStore};
use storage::api::{
    AtomicBlobPort, DeleteOptions, DeleteOutcome, Generation, PromoteOutcome, QuarantineOutcome,
    QuarantineReason, ReadOutcome, StorageError, StorageKey, TransactionScope, WriteOptions,
    WriteReceipt,
};

#[derive(Default)]
struct RecordingBlob {
    reads: Mutex<Vec<Generation>>,
    quarantines: Mutex<Vec<(Generation, TransactionScope, QuarantineReason)>>,
    writes: Mutex<Vec<Vec<u8>>>,
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
        _options: WriteOptions,
    ) -> Result<WriteReceipt, StorageError> {
        self.writes.lock().unwrap().push(bytes.to_vec());
        Ok(WriteReceipt::committed(None))
    }
    async fn promote_previous(&self, _key: &StorageKey) -> Result<PromoteOutcome, StorageError> {
        Ok(PromoteOutcome::AlreadyPromoted)
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
        _options: DeleteOptions,
    ) -> Result<DeleteOutcome, StorageError> {
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
    assert_eq!(*blob.writes.lock().unwrap(), vec![b"canonical".to_vec()]);
    assert_eq!(
        *blob.quarantines.lock().unwrap(),
        vec![(
            Generation::Previous,
            TransactionScope::Blob,
            QuarantineReason::DecoderRejected
        )]
    );
}

#[test]
fn adapter_rejects_unsafe_session_id_before_touching_storage() {
    let blob = Arc::new(RecordingBlob::default());
    assert!(AtomicBlobSessionStore::new(blob, "../escape").is_err());
}
