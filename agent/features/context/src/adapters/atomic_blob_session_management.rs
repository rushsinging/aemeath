use std::sync::Arc;

use async_trait::async_trait;
use storage::api::{AtomicBlobPort, StorageNamespace};

use crate::adapters::{AtomicBlobSessionStore, LegacySessionDecoder};
use crate::application::{SessionLoadError, SessionPersistenceService};
use crate::domain::session::{
    now_iso, session_matches_project, CanonicalSession, SessionCodec, SessionListEntry,
    SessionManagementError, SessionMetadataUpdate,
};
use crate::ports::SessionManagementPort;

pub struct AtomicBlobSessionManagement {
    blob: Arc<dyn AtomicBlobPort>,
}

impl AtomicBlobSessionManagement {
    pub fn new(blob: Arc<dyn AtomicBlobPort>) -> Self {
        Self { blob }
    }

    fn store(&self, id: &str) -> Result<Arc<AtomicBlobSessionStore>, SessionManagementError> {
        AtomicBlobSessionStore::new(Arc::clone(&self.blob), id)
            .map(Arc::new)
            .map_err(|error| SessionManagementError::Storage(error.to_string()))
    }

    fn persistence(&self, id: &str) -> Result<SessionPersistenceService, SessionManagementError> {
        Ok(SessionPersistenceService::new(
            self.store(id)?,
            Arc::new(LegacySessionDecoder),
        ))
    }

    async fn load_canonical(&self, id: &str) -> Result<CanonicalSession, SessionManagementError> {
        self.persistence(id)?
            .load()
            .await
            .map_err(|error| map_load(id, error))
    }
}

fn map_load(id: &str, error: SessionLoadError) -> SessionManagementError {
    match error {
        SessionLoadError::NotFound => SessionManagementError::NotFound(id.to_string()),
        SessionLoadError::NoDecodableGeneration => SessionManagementError::Corrupt(id.to_string()),
        SessionLoadError::UnsupportedFutureVersion { version, .. } => {
            SessionManagementError::UnsupportedFutureVersion(version)
        }
        other => SessionManagementError::Storage(other.to_string()),
    }
}

#[async_trait]
impl SessionManagementPort for AtomicBlobSessionManagement {
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
        let entries = self
            .blob
            .list_primary(StorageNamespace::Session)
            .await
            .map_err(|error| SessionManagementError::Storage(error.to_string()))?;
        let mut sessions = Vec::new();
        for entry in entries {
            let Some(id) = entry
                .key()
                .segments()
                .first()
                .map(|segment| segment.as_str())
            else {
                continue;
            };
            if let Ok(session) = self.load_canonical(id).await {
                if session_matches_project(&session, project) {
                    sessions.push(SessionListEntry::from_canonical(&session));
                }
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
        self.persistence(&session.id)?
            .save(&session)
            .await
            .map_err(|error| SessionManagementError::Storage(error.to_string()))?;
        Ok(SessionListEntry::from_canonical(&session))
    }

    async fn update_metadata_for_project(
        &self,
        id: &str,
        project: &share::session_types::ProjectIdentity,
        update: SessionMetadataUpdate,
    ) -> Result<SessionListEntry, SessionManagementError> {
        let mut session = self.load_for_project(id, project).await?;
        update.apply(&mut session.metadata);
        session.updated_at = now_iso();
        self.persistence(id)?
            .save(&session)
            .await
            .map_err(|error| SessionManagementError::Storage(error.to_string()))?;
        Ok(SessionListEntry::from_canonical(&session))
    }

    async fn delete_for_project(
        &self,
        id: &str,
        project: &share::session_types::ProjectIdentity,
    ) -> Result<(), SessionManagementError> {
        self.load_for_project(id, project).await?;
        let store = self.store(id)?;
        let outcome = store
            .delete_all()
            .await
            .map_err(|error| SessionManagementError::Storage(error.to_string()))?;
        if !outcome.deleted_primary() && !outcome.deleted_previous() {
            return Err(SessionManagementError::NotFound(id.to_string()));
        }
        Ok(())
    }
}
