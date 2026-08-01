use std::sync::Arc;

use async_trait::async_trait;
use storage::api::{AtomicBlobPort, AtomicDatasetPort, StorageNamespace};

use crate::adapters::{
    AtomicBlobSessionManagement, DatasetCanonicalSessionWriter, DatasetSessionReader,
};
use crate::domain::session::{
    now_iso, session_matches_project, CanonicalSession, SessionCodec, SessionListEntry,
    SessionManagementError, SessionMetadataUpdate,
};
use crate::ports::SessionManagementPort;

pub struct DatasetSessionManagement {
    dataset: Arc<dyn AtomicDatasetPort>,
    legacy: Arc<AtomicBlobSessionManagement>,
    reader: DatasetSessionReader,
    writer: DatasetCanonicalSessionWriter,
}

impl DatasetSessionManagement {
    pub fn new(dataset: Arc<dyn AtomicDatasetPort>, legacy_blob: Arc<dyn AtomicBlobPort>) -> Self {
        Self {
            reader: DatasetSessionReader::new(Arc::clone(&dataset), Some(Arc::clone(&legacy_blob))),
            writer: DatasetCanonicalSessionWriter::new(Arc::clone(&dataset)),
            dataset,
            legacy: Arc::new(AtomicBlobSessionManagement::new(legacy_blob)),
        }
    }

    async fn load_canonical(&self, id: &str) -> Result<CanonicalSession, SessionManagementError> {
        self.reader.load(id).await.map_err(map_reader_error)
    }
}

fn map_reader_error(
    error: crate::domain::session::SessionGenerationWireError,
) -> SessionManagementError {
    match error {
        crate::domain::session::SessionGenerationWireError::UnsupportedFutureVersion {
            version,
            ..
        } => SessionManagementError::UnsupportedFutureVersion(version),
        other => SessionManagementError::Corrupt(other.to_string()),
    }
}

#[async_trait]
impl SessionManagementPort for DatasetSessionManagement {
    async fn load_for_project(
        &self,
        id: &str,
        project: &share::session_types::ProjectIdentity,
    ) -> Result<CanonicalSession, SessionManagementError> {
        let session = self.load_canonical(id).await?;
        if session_matches_project(&session, project) {
            Ok(session)
        } else {
            Err(SessionManagementError::ProjectMismatch(id.to_string()))
        }
    }

    async fn list_for_project(
        &self,
        project: &share::session_types::ProjectIdentity,
    ) -> Result<Vec<SessionListEntry>, SessionManagementError> {
        let dataset_keys = self
            .dataset
            .list_datasets(StorageNamespace::Session)
            .await
            .map_err(|error| SessionManagementError::Storage(error.to_string()))?;
        let mut sessions = Vec::new();
        for dataset_key in dataset_keys {
            let Some(session_id) = dataset_key
                .segments()
                .first()
                .map(|segment| segment.as_str().strip_suffix(".dataset"))
                .flatten()
            else {
                continue;
            };
            if let Ok(session) = self.load_for_project(session_id, project).await {
                sessions.push(SessionListEntry::from_canonical(&session));
            }
        }
        for entry in self.legacy.list_for_project(project).await? {
            if !sessions.iter().any(|session| session.id == entry.id) {
                sessions.push(entry);
            }
        }
        sessions.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
        Ok(sessions)
    }

    async fn export_for_project(
        &self,
        id: &str,
        project: &share::session_types::ProjectIdentity,
    ) -> Result<Vec<u8>, SessionManagementError> {
        let session = self.load_for_project(id, project).await?;
        SessionCodec::encode(&session)
            .map_err(|error| SessionManagementError::Storage(error.to_string()))
    }

    async fn import_for_project(
        &self,
        bytes: &[u8],
        project: &share::session_types::ProjectIdentity,
    ) -> Result<SessionListEntry, SessionManagementError> {
        let decoded = crate::adapters::decode_session(bytes).map_err(|error| match error {
            crate::domain::session::SessionCodecError::UnsupportedFutureVersion {
                version, ..
            } => SessionManagementError::UnsupportedFutureVersion(version),
            other => SessionManagementError::Corrupt(other.to_string()),
        })?;
        let session = decoded.session;
        if !session_matches_project(&session, project) {
            return Err(SessionManagementError::ProjectMismatch(session.id));
        }
        self.writer
            .save_initial(&session)
            .await
            .map_err(SessionManagementError::Storage)?;
        Ok(SessionListEntry::from_canonical(&session))
    }

    async fn update_metadata_for_project(
        &self,
        id: &str,
        project: &share::session_types::ProjectIdentity,
        update: SessionMetadataUpdate,
    ) -> Result<SessionListEntry, SessionManagementError> {
        let before = self.load_for_project(id, project).await?;
        let mut after = before.clone();
        update.apply(&mut after.metadata);
        after.updated_at = now_iso();
        after.revision += 1;
        self.writer
            .save_incremental(&before, &after)
            .await
            .map_err(SessionManagementError::Storage)?;
        Ok(SessionListEntry::from_canonical(&after))
    }

    async fn delete_for_project(
        &self,
        id: &str,
        project: &share::session_types::ProjectIdentity,
    ) -> Result<(), SessionManagementError> {
        self.load_for_project(id, project).await?;
        let dataset_key = super::dataset_session_writer::session_dataset_key(id)
            .map_err(|error| SessionManagementError::Storage(error.to_string()))?;
        let outcome = self
            .dataset
            .delete_all_generations(&dataset_key, storage::api::DeleteOptions::default())
            .await
            .map_err(|error| SessionManagementError::Storage(error.to_string()))?;
        let legacy_outcome = self.legacy.delete_for_project(id, project).await.err();
        if !outcome.deleted_primary() && !outcome.deleted_previous() && legacy_outcome.is_some() {
            return Err(SessionManagementError::NotFound(id.to_string()));
        }
        Ok(())
    }
}
