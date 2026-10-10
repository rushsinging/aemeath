use std::sync::Arc;

use async_trait::async_trait;
use storage::{AtomicBlobPort, AtomicDatasetPort, SafePathSegmentData, StorageNamespaceData};

use crate::adapters::{
    AtomicBlobSessionManagement, DatasetCanonicalSessionWriter, DatasetSessionReader,
};
use crate::domain::session::{
    now_iso, project_dir_segment, session_matches_project, CanonicalSession, SessionCodec,
    SessionListEntryData, SessionMetadataUpdateData,
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
            dataset: Arc::clone(&dataset),
            legacy: Arc::new(AtomicBlobSessionManagement::new(Arc::clone(&legacy_blob))),
            reader: DatasetSessionReader::new(Arc::clone(&dataset), Some(Arc::clone(&legacy_blob))),
            writer: DatasetCanonicalSessionWriter::new(Arc::clone(&dataset)),
        }
    }

    async fn load_canonical(
        &self,
        project_dir: Option<&SafePathSegmentData>,
        id: &str,
    ) -> Result<CanonicalSession, share::error::DomainError> {
        self.reader
            .load(project_dir, id)
            .await
            .map_err(map_reader_error)
    }
}

fn map_reader_error(
    error: crate::domain::session::SessionGenerationWireError,
) -> share::error::DomainError {
    match error {
        crate::domain::session::SessionGenerationWireError::UnsupportedFutureVersion {
            version,
            ..
        } => share::error::DomainError::invalid(
            "context",
            format!("Session schema 版本过新：{version}"),
        )
        .with_code(crate::error_codes::SESSION_UNSUPPORTED_VERSION),
        other => {
            share::error::DomainError::invalid("context", format!("Session 数据损坏：{other}"))
                .with_code(crate::error_codes::SESSION_CORRUPT)
        }
    }
}

#[async_trait]
impl SessionManagementPort for DatasetSessionManagement {
    async fn load_for_project(
        &self,
        id: &str,
        project: &share::session_types::ProjectIdentityData,
    ) -> Result<CanonicalSession, share::error::DomainError> {
        let project_dir = project_dir_segment(project);
        let session = self.load_canonical(Some(&project_dir), id).await?;
        if session_matches_project(&session, project) {
            Ok(session)
        } else {
            Err(share::error::DomainError::invalid(
                "context",
                format!("Session 不属于当前项目：{id}"),
            )
            .with_code(crate::error_codes::SESSION_PROJECT_MISMATCH))
        }
    }

    async fn load_for_resume(
        &self,
        id: &str,
        project: &share::session_types::ProjectIdentityData,
    ) -> Result<crate::domain::session::SessionResumeLoad, share::error::DomainError> {
        let project_dir = project_dir_segment(project);
        let prepared = self
            .reader
            .load_for_resume(Some(&project_dir), id)
            .await
            .map_err(map_reader_error)?;
        if !session_matches_project(&prepared.active_session, project) {
            return Err(share::error::DomainError::invalid(
                "context",
                format!("Session 不属于当前项目：{id}"),
            )
            .with_code(crate::error_codes::SESSION_PROJECT_MISMATCH));
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
    ) -> Result<crate::domain::session::DisplayHistoryStepWindowData, share::error::DomainError>
    {
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
    ) -> Result<Vec<SessionListEntryData>, share::error::DomainError> {
        let project_dir = project_dir_segment(project);
        let dataset_keys = self
            .dataset
            .list_datasets(StorageNamespaceData::Session)
            .await
            .map_err(|error| {
                share::error::DomainError::storage("context", error.to_string())
                    .with_code(crate::error_codes::SESSION_STORAGE)
            })?;
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
                sessions.push(SessionListEntryData::from_canonical(&session));
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
    ) -> Result<Vec<u8>, share::error::DomainError> {
        let session = self.load_for_project(id, project).await?;
        SessionCodec::encode(&session).map_err(|error| {
            share::error::DomainError::storage("context", error.to_string())
                .with_code(crate::error_codes::SESSION_STORAGE)
        })
    }

    async fn import_for_project(
        &self,
        bytes: &[u8],
        project: &share::session_types::ProjectIdentityData,
    ) -> Result<SessionListEntryData, share::error::DomainError> {
        let decoded = crate::adapters::decode_session(bytes).map_err(|error| match error {
            crate::domain::session::SessionCodecError::UnsupportedFutureVersion {
                version, ..
            } => share::error::DomainError::invalid(
                "context",
                format!("Session schema 版本过新：{version}"),
            )
            .with_code(crate::error_codes::SESSION_UNSUPPORTED_VERSION),
            other => {
                share::error::DomainError::invalid("context", format!("Session 数据损坏：{other}"))
                    .with_code(crate::error_codes::SESSION_CORRUPT)
            }
        })?;
        let session = decoded.session;
        if !session_matches_project(&session, project) {
            return Err(share::error::DomainError::invalid(
                "context",
                format!("Session 不属于当前项目：{}", session.id),
            )
            .with_code(crate::error_codes::SESSION_PROJECT_MISMATCH));
        }
        self.writer.save_initial(&session).await.map_err(|error| {
            share::error::DomainError::storage("context", error.to_string())
                .with_code(crate::error_codes::SESSION_STORAGE)
        })?;
        Ok(SessionListEntryData::from_canonical(&session))
    }

    async fn update_metadata_for_project(
        &self,
        id: &str,
        project: &share::session_types::ProjectIdentityData,
        update: SessionMetadataUpdateData,
    ) -> Result<SessionListEntryData, share::error::DomainError> {
        let before = self.load_for_project(id, project).await?;
        let mut after = before.clone();
        update.apply(&mut after.metadata);
        after.updated_at = now_iso();
        after.revision += 1;
        self.writer
            .save_incremental(&before, &after)
            .await
            .map_err(|error| {
                share::error::DomainError::storage("context", error.to_string())
                    .with_code(crate::error_codes::SESSION_STORAGE)
            })?;
        Ok(SessionListEntryData::from_canonical(&after))
    }

    async fn delete_for_project(
        &self,
        id: &str,
        project: &share::session_types::ProjectIdentityData,
    ) -> Result<(), share::error::DomainError> {
        self.load_for_project(id, project).await?;
        let dataset_key =
            super::dataset_session_writer::session_dataset_key(id).map_err(|error| {
                share::error::DomainError::storage("context", error.to_string())
                    .with_code(crate::error_codes::SESSION_STORAGE)
            })?;
        let outcome = self
            .dataset
            .delete_all_generations(&dataset_key, storage::DeleteOptionsData::default())
            .await
            .map_err(|error| {
                share::error::DomainError::storage("context", error.to_string())
                    .with_code(crate::error_codes::SESSION_STORAGE)
            })?;
        let legacy_outcome = self.legacy.delete_for_project(id, project).await.err();
        if !outcome.deleted_primary() && !outcome.deleted_previous() && legacy_outcome.is_some() {
            return Err(share::error::DomainError::invalid(
                "context",
                format!("Session 不存在：{id}"),
            )
            .with_code(crate::error_codes::SESSION_NOT_FOUND));
        }
        Ok(())
    }
}
