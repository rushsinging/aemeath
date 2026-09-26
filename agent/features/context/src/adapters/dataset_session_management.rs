use std::sync::Arc;

use async_trait::async_trait;
use storage::{AtomicBlobPort, AtomicDatasetPort, SafePathSegment, StorageNamespace};

use crate::adapters::{
    AtomicBlobSessionManagement, DatasetCanonicalSessionWriter, DatasetSessionReader,
};
use crate::domain::session::{
    now_iso, project_dir_segment, session_matches_project, CanonicalSession, SessionCodec,
    SessionListEntry, SessionManagementError, SessionMetadataUpdate,
};
use crate::ports::SessionManagementPort;

pub struct DatasetSessionManagement {
    dataset: Arc<dyn AtomicDatasetPort>,
    legacy: Arc<AtomicBlobSessionManagement>,
    receipt_blob: Arc<dyn AtomicBlobPort>,
    reader: DatasetSessionReader,
    writer: DatasetCanonicalSessionWriter,
}

impl DatasetSessionManagement {
    pub fn new(dataset: Arc<dyn AtomicDatasetPort>, legacy_blob: Arc<dyn AtomicBlobPort>) -> Self {
        Self {
            dataset: Arc::clone(&dataset),
            legacy: Arc::new(AtomicBlobSessionManagement::new(Arc::clone(&legacy_blob))),
            receipt_blob: legacy_blob.clone(),
            reader: DatasetSessionReader::new(Arc::clone(&dataset), Some(Arc::clone(&legacy_blob))),
            writer: DatasetCanonicalSessionWriter::new(Arc::clone(&dataset)),
        }
    }

    pub fn accepted_input_writer(&self) -> crate::adapters::AtomicBlobAcceptedInputWriter {
        crate::adapters::AtomicBlobAcceptedInputWriter::new(Arc::clone(&self.receipt_blob))
    }
    pub fn tool_receipt_writer(&self) -> crate::adapters::AtomicBlobToolReceiptWriter {
        crate::adapters::AtomicBlobToolReceiptWriter::new(Arc::clone(&self.receipt_blob))
    }
    async fn load_canonical(
        &self,
        project_dir: Option<&SafePathSegment>,
        id: &str,
    ) -> Result<CanonicalSession, SessionManagementError> {
        self.reader
            .load(project_dir, id)
            .await
            .map_err(map_reader_error)
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
        project: &share::session_types::ProjectIdentityData,
    ) -> Result<CanonicalSession, SessionManagementError> {
        let project_dir = project_dir_segment(project);
        let session = self.load_canonical(Some(&project_dir), id).await?;
        if session_matches_project(&session, project) {
            Ok(session)
        } else {
            Err(SessionManagementError::ProjectMismatch(id.to_string()))
        }
    }

    async fn load_for_resume(
        &self,
        id: &str,
        project: &share::session_types::ProjectIdentityData,
    ) -> Result<crate::domain::session::SessionResumeLoad, SessionManagementError> {
        let project_dir = project_dir_segment(project);
        let prepared = self
            .reader
            .load_for_resume(Some(&project_dir), id)
            .await
            .map_err(map_reader_error)?;
        if !session_matches_project(&prepared.active_session, project) {
            return Err(SessionManagementError::ProjectMismatch(id.to_string()));
        }
        Ok(crate::domain::session::SessionResumeLoad {
            active_session: prepared.active_session,
            display_history: Some(prepared.display_history),
        })
    }

    async fn load_display_history_steps(
        &self,
        id: &str,
        project: &share::session_types::ProjectIdentityData,
        generation_revision: u64,
        member_names: &[String],
    ) -> Result<crate::domain::session::DisplayHistoryStepWindow, SessionManagementError> {
        self.load_for_project(id, project).await?;
        let project_dir = project_dir_segment(project);
        self.reader
            .load_display_history_steps(Some(&project_dir), id, generation_revision, member_names)
            .await
            .map_err(map_reader_error)
    }

    /// 按 project 分目录布局：dataset key 首段为本项目目录段的直接采纳；
    /// 平铺单段 key（迁移前遗留）加载后按 identity 过滤。跨项目 session
    /// 永不出现在结果中，且不做全目录扫描加载。
    async fn list_for_project(
        &self,
        project: &share::session_types::ProjectIdentityData,
    ) -> Result<Vec<SessionListEntry>, SessionManagementError> {
        let project_dir = project_dir_segment(project);
        let dataset_keys = self
            .dataset
            .list_datasets(StorageNamespace::Session)
            .await
            .map_err(|error| SessionManagementError::Storage(error.to_string()))?;
        let mut sessions = Vec::new();
        for dataset_key in dataset_keys {
            let segments = dataset_key.segments();
            let session_id = if segments.len() == 2 && segments[0] == project_dir {
                match segments[1].as_str().strip_suffix(".dataset") {
                    Some(session_id) => session_id.to_string(),
                    None => continue,
                }
            } else if segments.len() == 1 {
                match segments[0].as_str().strip_suffix(".dataset") {
                    Some(session_id) => session_id.to_string(),
                    None => continue,
                }
            } else {
                continue;
            };
            if let Ok(session) = self.load_canonical(Some(&project_dir), &session_id).await {
                if segments.len() == 1 && !session_matches_project(&session, project) {
                    continue;
                }
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
        project: &share::session_types::ProjectIdentityData,
    ) -> Result<Vec<u8>, SessionManagementError> {
        let session = self.load_for_project(id, project).await?;
        SessionCodec::encode(&session)
            .map_err(|error| SessionManagementError::Storage(error.to_string()))
    }

    async fn import_for_project(
        &self,
        bytes: &[u8],
        project: &share::session_types::ProjectIdentityData,
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
        project: &share::session_types::ProjectIdentityData,
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
        project: &share::session_types::ProjectIdentityData,
    ) -> Result<(), SessionManagementError> {
        self.load_for_project(id, project).await?;
        let dataset_key = super::dataset_session_writer::session_dataset_key(id)
            .map_err(|error| SessionManagementError::Storage(error.to_string()))?;
        let outcome = self
            .dataset
            .delete_all_generations(&dataset_key, storage::DeleteOptions::default())
            .await
            .map_err(|error| SessionManagementError::Storage(error.to_string()))?;
        let legacy_outcome = self.legacy.delete_for_project(id, project).await.err();
        if !outcome.deleted_primary() && !outcome.deleted_previous() && legacy_outcome.is_some() {
            return Err(SessionManagementError::NotFound(id.to_string()));
        }
        Ok(())
    }
}
