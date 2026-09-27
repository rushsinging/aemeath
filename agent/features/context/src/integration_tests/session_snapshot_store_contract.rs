use std::sync::{Arc, Mutex};

use crate::AtomicBlobSessionStore;
use crate::{SessionGeneration, SessionSnapshotStore};
use async_trait::async_trait;
use storage::{
    AtomicBlobPort, DeleteOptionsData, DeleteOutcomeData, GenerationData, PromoteOutcomeData,
    QuarantineOutcomeData, QuarantineReason, ReadOutcomeData, StorageError, StorageKeyData,
    TransactionScopeData, WriteOptionsData, WriteReceiptData,
};

struct RecordingBlob {
    reads: Mutex<Vec<GenerationData>>,
    quarantines: Mutex<Vec<(GenerationData, TransactionScopeData, QuarantineReason)>>,
    writes: Mutex<Vec<(Vec<u8>, WriteOptionsData)>>,
    promote_outcome: Mutex<PromoteOutcomeData>,
    delete_options: Mutex<Vec<DeleteOptionsData>>,
}

impl Default for RecordingBlob {
    fn default() -> Self {
        Self {
            reads: Mutex::new(Vec::new()),
            quarantines: Mutex::new(Vec::new()),
            writes: Mutex::new(Vec::new()),
            promote_outcome: Mutex::new(PromoteOutcomeData::AlreadyPromoted),
            delete_options: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl AtomicBlobPort for RecordingBlob {
    async fn read(
        &self,
        _key: &StorageKeyData,
        generation: GenerationData,
    ) -> Result<ReadOutcomeData, StorageError> {
        self.reads.lock().unwrap().push(generation);
        Ok(ReadOutcomeData::NotFound)
    }
    async fn write_atomic(
        &self,
        _key: &StorageKeyData,
        bytes: &[u8],
        options: WriteOptionsData,
    ) -> Result<WriteReceiptData, StorageError> {
        self.writes.lock().unwrap().push((bytes.to_vec(), options));
        Ok(WriteReceiptData::committed(None))
    }
    async fn promote_previous(
        &self,
        _key: &StorageKeyData,
    ) -> Result<PromoteOutcomeData, StorageError> {
        Ok(*self.promote_outcome.lock().unwrap())
    }
    async fn quarantine(
        &self,
        _key: &StorageKeyData,
        generation: GenerationData,
        scope: TransactionScopeData,
        reason: QuarantineReason,
    ) -> Result<QuarantineOutcomeData, StorageError> {
        self.quarantines
            .lock()
            .unwrap()
            .push((generation, scope, reason));
        Ok(QuarantineOutcomeData::already_absent(
            generation, scope, reason,
        ))
    }
    async fn delete_all_generations(
        &self,
        _key: &StorageKeyData,
        options: DeleteOptionsData,
    ) -> Result<DeleteOutcomeData, StorageError> {
        self.delete_options.lock().unwrap().push(options);
        Ok(DeleteOutcomeData::new(false, false, false))
    }

    async fn list_primary(
        &self,
        _namespace: storage::StorageNamespaceData,
    ) -> Result<Vec<storage::StorageEntryData>, StorageError> {
        Ok(Vec::new())
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
    assert_eq!(*blob.reads.lock().unwrap(), vec![GenerationData::Primary]);
    let writes = blob.writes.lock().unwrap();
    assert_eq!(writes.len(), 1);
    assert_eq!(writes[0].0, b"canonical");
    assert_eq!(
        writes[0].1.durability(),
        storage::DurabilityData::ProcessCrashSafe
    );
    assert_eq!(
        *blob.quarantines.lock().unwrap(),
        vec![(
            GenerationData::Previous,
            TransactionScopeData::Blob,
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
    *blob.promote_outcome.lock().unwrap() = PromoteOutcomeData::NotFound;
    let store = AtomicBlobSessionStore::new(blob, "session-1").unwrap();

    let error = store.promote_previous().await.unwrap_err();
    assert!(error.to_string().contains("not found"));
}

#[test]
fn adapter_rejects_unsafe_session_id_before_touching_storage() {
    let blob = Arc::new(RecordingBlob::default());
    assert!(AtomicBlobSessionStore::new(blob, "../escape").is_err());
}
