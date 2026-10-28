use std::sync::Arc;

use crate::AtomicBlobSessionStore;
use crate::{SessionGeneration, SessionSnapshotStore};
use storage::{GenerationData, PromoteOutcomeData, QuarantineReason, TransactionScopeData};

#[tokio::test]
async fn adapter_maps_context_operations_to_session_atomic_blob_contract() {
    let blob = Arc::new(storage::test_support::InMemoryBlobPort::new());
    let store = AtomicBlobSessionStore::new(blob.clone(), "session-1").unwrap();
    assert!(store
        .read(SessionGeneration::Primary)
        .await
        .unwrap()
        .is_none());
    store.write(b"canonical").await.unwrap();
    store.quarantine(SessionGeneration::Previous).await.unwrap();
    assert_eq!(blob.reads(), vec![GenerationData::Primary]);
    let writes = blob.writes();
    assert_eq!(writes.len(), 1);
    assert_eq!(writes[0].1.as_slice(), b"canonical");
    assert_eq!(
        writes[0].2.durability(),
        storage::DurabilityData::ProcessCrashSafe
    );
    assert_eq!(
        blob.quarantines(),
        vec![(
            GenerationData::Previous,
            TransactionScopeData::Blob,
            QuarantineReason::DecoderRejected
        )]
    );
}

#[tokio::test]
async fn adapter_maps_promote_and_delete_to_atomic_blob_contract() {
    let blob = Arc::new(storage::test_support::InMemoryBlobPort::new());
    let store = AtomicBlobSessionStore::new(blob.clone(), "session-1").unwrap();

    store.promote_previous().await.unwrap();
    store.delete_all().await.unwrap();

    let options = blob.delete_options();
    assert_eq!(options.len(), 1);
    assert!(options[0].include_quarantine());
}

#[tokio::test]
async fn adapter_reports_missing_previous_generation() {
    let blob = Arc::new(storage::test_support::InMemoryBlobPort::new());
    blob.set_promote_outcome(PromoteOutcomeData::NotFound);
    let store = AtomicBlobSessionStore::new(blob, "session-1").unwrap();

    let error = store.promote_previous().await.unwrap_err();
    assert!(error.to_string().contains("not found"));
}

#[test]
fn adapter_rejects_unsafe_session_id_before_touching_storage() {
    let blob = Arc::new(storage::test_support::InMemoryBlobPort::new());
    assert!(AtomicBlobSessionStore::new(blob, "../escape").is_err());
}
