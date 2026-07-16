use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use context::application::{SessionLoadError, SessionPersistenceService};
use context::ports::{SessionGeneration, SessionSnapshotStore, SessionStoreError};

#[derive(Default)]
struct JourneyStore {
    primary: Mutex<Option<Vec<u8>>>,
    previous: Mutex<Option<Vec<u8>>>,
    writes: Mutex<Vec<Vec<u8>>>,
    promoted: Mutex<bool>,
}

#[async_trait]
impl SessionSnapshotStore for JourneyStore {
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
        *self.promoted.lock().unwrap() = true;
        Ok(())
    }
    async fn quarantine(&self, _generation: SessionGeneration) -> Result<(), SessionStoreError> {
        Ok(())
    }
}

#[tokio::test]
async fn legacy_load_save_reload_is_canonical() {
    let store = Arc::new(JourneyStore::default());
    *store.primary.lock().unwrap() = Some(include_bytes!("fixtures/session/legacy.json").to_vec());
    let service = SessionPersistenceService::new(store.clone());
    let session = service.load().await.unwrap();
    service.save(&session).await.unwrap();
    let canonical = store.writes.lock().unwrap()[0].clone();
    assert!(String::from_utf8(canonical.clone())
        .unwrap()
        .contains("\"schema_version\": 1"));
    *store.primary.lock().unwrap() = Some(canonical);
    assert_eq!(service.load().await.unwrap().id, "legacy-fixture");
}

#[tokio::test]
async fn future_fixture_is_preserved_without_write() {
    let store = Arc::new(JourneyStore::default());
    let future = include_bytes!("fixtures/session/future.json").to_vec();
    *store.primary.lock().unwrap() = Some(future.clone());
    assert!(
        matches!(SessionPersistenceService::new(store.clone()).load().await, Err(SessionLoadError::UnsupportedFutureVersion { original_bytes, .. }) if original_bytes == future)
    );
    assert!(store.writes.lock().unwrap().is_empty());
}
