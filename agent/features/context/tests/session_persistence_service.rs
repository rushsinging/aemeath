use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use context::adapters::LegacySessionDecoder;
use context::application::{SessionLoadError, SessionPersistenceService};
use context::domain::session::{CanonicalSession, SessionCodec};
use context::ports::{SessionGeneration, SessionSnapshotStore, SessionStoreError};

#[derive(Default)]
struct ScriptedStore {
    primary: Mutex<Option<Vec<u8>>>,
    previous: Mutex<Option<Vec<u8>>>,
    promoted: Mutex<usize>,
    quarantined: Mutex<Vec<SessionGeneration>>,
    writes: Mutex<Vec<Vec<u8>>>,
}

#[async_trait]
impl SessionSnapshotStore for ScriptedStore {
    async fn read(
        &self,
        generation: SessionGeneration,
    ) -> Result<Option<Vec<u8>>, SessionStoreError> {
        Ok(match generation {
            SessionGeneration::Primary => self.primary.lock().unwrap().clone(),
            SessionGeneration::Previous => self.previous.lock().unwrap().clone(),
        })
    }

    async fn write(&self, bytes: &[u8]) -> Result<(), SessionStoreError> {
        self.writes.lock().unwrap().push(bytes.to_vec());
        Ok(())
    }

    async fn promote_previous(&self) -> Result<(), SessionStoreError> {
        *self.promoted.lock().unwrap() += 1;
        Ok(())
    }

    async fn quarantine(&self, generation: SessionGeneration) -> Result<(), SessionStoreError> {
        self.quarantined.lock().unwrap().push(generation);
        Ok(())
    }
}

#[tokio::test]
async fn valid_primary_is_returned_without_recovery_side_effects() {
    let store = Arc::new(ScriptedStore::default());
    *store.primary.lock().unwrap() =
        Some(SessionCodec::encode(&CanonicalSession::fixture("ok")).unwrap());
    let loaded = SessionPersistenceService::new(store.clone(), Arc::new(LegacySessionDecoder))
        .load()
        .await
        .unwrap();
    assert_eq!(loaded.id, "ok");
    assert_eq!(*store.promoted.lock().unwrap(), 0);
}

#[tokio::test]
async fn invalid_primary_uses_valid_previous_and_promotes() {
    let store = Arc::new(ScriptedStore::default());
    *store.primary.lock().unwrap() = Some(b"broken".to_vec());
    *store.previous.lock().unwrap() =
        Some(SessionCodec::encode(&CanonicalSession::fixture("old")).unwrap());
    let loaded = SessionPersistenceService::new(store.clone(), Arc::new(LegacySessionDecoder))
        .load()
        .await
        .unwrap();
    assert_eq!(loaded.id, "old");
    assert_eq!(*store.promoted.lock().unwrap(), 1);
    assert_eq!(
        *store.quarantined.lock().unwrap(),
        vec![SessionGeneration::Primary]
    );
}

#[tokio::test]
async fn both_invalid_generations_are_quarantined() {
    let store = Arc::new(ScriptedStore::default());
    *store.primary.lock().unwrap() = Some(b"broken-primary".to_vec());
    *store.previous.lock().unwrap() = Some(b"broken-previous".to_vec());
    assert!(matches!(
        SessionPersistenceService::new(store.clone(), Arc::new(LegacySessionDecoder))
            .load()
            .await,
        Err(SessionLoadError::NoDecodableGeneration)
    ));
    assert_eq!(
        store.quarantined.lock().unwrap().as_slice(),
        &[SessionGeneration::Primary, SessionGeneration::Previous]
    );
}

#[tokio::test]
async fn future_primary_is_preserved_and_never_falls_back_or_writes() {
    let store = Arc::new(ScriptedStore::default());
    let future = br#"{"schema_version":999,"id":"future"}"#.to_vec();
    *store.primary.lock().unwrap() = Some(future.clone());
    *store.previous.lock().unwrap() =
        Some(SessionCodec::encode(&CanonicalSession::fixture("old")).unwrap());
    assert!(matches!(
        SessionPersistenceService::new(store.clone(), Arc::new(LegacySessionDecoder)).load().await,
        Err(SessionLoadError::UnsupportedFutureVersion { original_bytes, .. }) if original_bytes == future
    ));
    assert!(store.quarantined.lock().unwrap().is_empty());
    assert!(store.writes.lock().unwrap().is_empty());
}
