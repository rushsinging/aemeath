use std::sync::Arc;

use async_trait::async_trait;
use storage::{
    AtomicBlobPort, DurabilityData, GenerationData, PromoteOutcomeData, QuarantineReason,
    ReadOutcomeData, SafePathSegmentData, StorageKeyData, StorageNamespaceData,
    TransactionScopeData, WriteOptionsData,
};

use crate::ports::{SessionGeneration, SessionSnapshotStore, SessionStoreError};

pub struct AtomicBlobSessionStore {
    blob: Arc<dyn AtomicBlobPort>,
    key: StorageKeyData,
}

impl AtomicBlobSessionStore {
    pub fn new(blob: Arc<dyn AtomicBlobPort>, session_id: &str) -> Result<Self, SessionStoreError> {
        let segment = session_id
            .parse::<SafePathSegmentData>()
            .map_err(|error| SessionStoreError(error.to_string()))?;
        let key = StorageKeyData::new(StorageNamespaceData::Session, vec![segment])
            .map_err(|error| SessionStoreError(error.to_string()))?;
        Ok(Self { blob, key })
    }

    /// 按 project 分目录的 store：key 为 `<project-dir>/<session-id>`。
    pub fn new_scoped(
        blob: Arc<dyn AtomicBlobPort>,
        project_dir: &SafePathSegmentData,
        session_id: &str,
    ) -> Result<Self, SessionStoreError> {
        let segment = session_id
            .parse::<SafePathSegmentData>()
            .map_err(|error| SessionStoreError(error.to_string()))?;
        let key = StorageKeyData::new(
            StorageNamespaceData::Session,
            vec![project_dir.clone(), segment],
        )
        .map_err(|error| SessionStoreError(error.to_string()))?;
        Ok(Self { blob, key })
    }

    /// 按显式段列表构造（legacy 兼容读取与迁移器复用）。
    pub fn from_key_segments(
        blob: Arc<dyn AtomicBlobPort>,
        raw_segments: Vec<String>,
    ) -> Result<Self, SessionStoreError> {
        let segments = raw_segments
            .into_iter()
            .map(|segment| {
                segment
                    .parse::<SafePathSegmentData>()
                    .map_err(|error| SessionStoreError(error.to_string()))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let key = StorageKeyData::new(StorageNamespaceData::Session, segments)
            .map_err(|error| SessionStoreError(error.to_string()))?;
        Ok(Self { blob, key })
    }

    fn storage_generation(generation: SessionGeneration) -> GenerationData {
        match generation {
            SessionGeneration::Primary => GenerationData::Primary,
            SessionGeneration::Previous => GenerationData::Previous,
        }
    }

    pub async fn delete_all(&self) -> Result<storage::DeleteOutcomeData, SessionStoreError> {
        self.blob
            .delete_all_generations(&self.key, storage::DeleteOptionsData::default())
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
            ReadOutcomeData::Found(read) => Ok(Some(read.bytes().to_vec())),
            ReadOutcomeData::NotFound => Ok(None),
        }
    }

    async fn write(&self, bytes: &[u8]) -> Result<(), SessionStoreError> {
        self.blob
            .write_atomic(
                &self.key,
                bytes,
                WriteOptionsData::new(DurabilityData::ProcessCrashSafe),
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
            PromoteOutcomeData::Promoted(_) | PromoteOutcomeData::AlreadyPromoted => Ok(()),
            PromoteOutcomeData::NotFound => Err(SessionStoreError(
                "previous Session generation not found".into(),
            )),
        }
    }

    async fn quarantine(&self, generation: SessionGeneration) -> Result<(), SessionStoreError> {
        self.blob
            .quarantine(
                &self.key,
                Self::storage_generation(generation),
                TransactionScopeData::Blob,
                QuarantineReason::DecoderRejected,
            )
            .await
            .map_err(|error| SessionStoreError(error.to_string()))?;
        Ok(())
    }
}
