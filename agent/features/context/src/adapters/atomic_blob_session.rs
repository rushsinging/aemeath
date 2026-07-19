use std::sync::Arc;

use async_trait::async_trait;
use storage::api::{
    AtomicBlobPort, Durability, Generation, PromoteOutcome, QuarantineReason, ReadOutcome,
    SafePathSegment, StorageKey, StorageNamespace, TransactionScope, WriteOptions,
};

use crate::ports::{SessionGeneration, SessionSnapshotStore, SessionStoreError};

pub struct AtomicBlobSessionStore {
    blob: Arc<dyn AtomicBlobPort>,
    key: StorageKey,
}

impl AtomicBlobSessionStore {
    pub fn new(blob: Arc<dyn AtomicBlobPort>, session_id: &str) -> Result<Self, SessionStoreError> {
        let segment = session_id
            .parse::<SafePathSegment>()
            .map_err(|error| SessionStoreError(error.to_string()))?;
        let key = StorageKey::new(StorageNamespace::Session, vec![segment])
            .map_err(|error| SessionStoreError(error.to_string()))?;
        Ok(Self { blob, key })
    }

    fn storage_generation(generation: SessionGeneration) -> Generation {
        match generation {
            SessionGeneration::Primary => Generation::Primary,
            SessionGeneration::Previous => Generation::Previous,
        }
    }

    pub async fn delete_all(&self) -> Result<storage::api::DeleteOutcome, SessionStoreError> {
        self.blob
            .delete_all_generations(&self.key, storage::api::DeleteOptions::default())
            .await
            .map_err(|error| SessionStoreError(error.to_string()))
    }
}

#[async_trait]
impl SessionSnapshotStore for AtomicBlobSessionStore {
    async fn read(
        &self,
        generation: SessionGeneration,
    ) -> Result<Option<Vec<u8>>, SessionStoreError> {
        match self
            .blob
            .read(&self.key, Self::storage_generation(generation))
            .await
            .map_err(|error| SessionStoreError(error.to_string()))?
        {
            ReadOutcome::Found(read) => Ok(Some(read.bytes().to_vec())),
            ReadOutcome::NotFound => Ok(None),
        }
    }

    async fn write(&self, bytes: &[u8]) -> Result<(), SessionStoreError> {
        self.blob
            .write_atomic(
                &self.key,
                bytes,
                WriteOptions::new(Durability::ProcessCrashSafe),
            )
            .await
            .map_err(|error| SessionStoreError(error.to_string()))?;
        Ok(())
    }

    async fn promote_previous(&self) -> Result<(), SessionStoreError> {
        match self
            .blob
            .promote_previous(&self.key)
            .await
            .map_err(|error| SessionStoreError(error.to_string()))?
        {
            PromoteOutcome::Promoted(_) | PromoteOutcome::AlreadyPromoted => Ok(()),
            PromoteOutcome::NotFound => Err(SessionStoreError(
                "previous Session generation not found".into(),
            )),
        }
    }

    async fn quarantine(&self, generation: SessionGeneration) -> Result<(), SessionStoreError> {
        self.blob
            .quarantine(
                &self.key,
                Self::storage_generation(generation),
                TransactionScope::Blob,
                QuarantineReason::DecoderRejected,
            )
            .await
            .map_err(|error| SessionStoreError(error.to_string()))?;
        Ok(())
    }
}
