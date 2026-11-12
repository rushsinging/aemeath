use crate::domain::session::project_list_item;
use std::sync::Arc;

use async_trait::async_trait;
use storage::{AtomicBlobPort, AtomicDatasetPort, SafePathSegmentData, StorageNamespaceData};

use crate::adapters::{
    AtomicBlobSessionManagement, DatasetCanonicalSessionWriter, DatasetSessionReader,
};
use crate::domain::session::{
    project_dir_segment, session_matches_project, CanonicalSession, SessionCodec,
    SessionManagementError, SessionMetadata,
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
    ) -> Result<CanonicalSession, SessionManagementError> {
        self.reader
            .load(project_dir, id)
            .await
            .map_err(|error| map_reader_error(id, error))
    }
}

/// dataset 读取错误 → 会话管理错误。
///
/// 「两处都不存在」是 `NotFound`，与「存在但 manifest/成员解码失败」的
/// `Corrupt` 严格区分：外部（startup `--resume`、终端会话恢复）据此给出
/// 正确提示，NEVER 把不存在的会话报成「数据损坏」。
fn map_reader_error(
    id: &str,
    error: crate::domain::session::SessionGenerationWireError,
) -> SessionManagementError {
    match error {
        crate::domain::session::SessionGenerationWireError::UnsupportedFutureVersion {
            version,
            ..
        } => SessionManagementError::UnsupportedFutureVersion(version),
        crate::domain::session::SessionGenerationWireError::NotFound => {
            SessionManagementError::NotFound(id.to_string())
        }
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
            .map_err(|error| map_reader_error(id, error))?;
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
    ) -> Result<crate::domain::session::DisplayHistoryStepWindowData, SessionManagementError> {
        self.load_for_project(id, project).await?;
        let project_dir = project_dir_segment(project);
        self.reader
            .load_display_history_steps(Some(&project_dir), id, generation_revision, member_names)
            .await
            .map_err(|error| map_reader_error(id, error))
    }

    /// 按 project 分目录布局：dataset key 首段为本项目目录段的直接采纳；
    /// 平铺单段 key（迁移前遗留）加载后按 identity 过滤。跨项目 session
    /// 永不出现在结果中，且不做全目录扫描加载。
    async fn list_for_project(
        &self,
        project: &share::session_types::ProjectIdentityData,
    ) -> Result<Vec<sdk::SessionItemList>, SessionManagementError> {
        let project_dir = project_dir_segment(project);
        let dataset_keys = self
            .dataset
            .list_datasets(StorageNamespaceData::Session)
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
                // #2160 P2：sub run 存档不进 session 枚举（路径已在 subruns/ 下被
                // 上面的段数解析跳过，这里按元数据再兜一层）。
                if session.metadata.origin == Some(crate::domain::session::SessionOrigin::SubRun) {
                    continue;
                }
                if segments.len() == 1 && !session_matches_project(&session, project) {
                    continue;
                }
                sessions.push(project_list_item(&session));
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
    ) -> Result<sdk::SessionItemList, SessionManagementError> {
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
        Ok(project_list_item(&session))
    }

    async fn update_metadata_for_project(
        &self,
        id: &str,
        project: &share::session_types::ProjectIdentityData,
        update: SessionMetadata,
    ) -> Result<sdk::SessionItemList, SessionManagementError> {
        let before = self.load_for_project(id, project).await?;
        let mut after = before.clone();
        after.metadata = update;
        after.updated_at = share::time::now_iso();
        after.revision += 1;
        self.writer
            .save_incremental(&before, &after)
            .await
            .map_err(SessionManagementError::Storage)?;
        Ok(project_list_item(&after))
    }

    async fn delete_for_project(
        &self,
        id: &str,
        project: &share::session_types::ProjectIdentityData,
    ) -> Result<(), SessionManagementError> {
        self.load_for_project(id, project).await?;
        // 按 project 分目录布局与旧平铺布局都要尝试删除：既有会话可能落在任一种。
        let project_dir = crate::domain::session::project_dir_segment(project);
        let scoped_key =
            super::dataset_session_writer::session_dataset_key_scoped(&project_dir, id)
                .map_err(|error| SessionManagementError::Storage(error.to_string()))?;
        let flat_key = super::dataset_session_writer::session_dataset_key(id)
            .map_err(|error| SessionManagementError::Storage(error.to_string()))?;
        let mut outcome = self
            .dataset
            .delete_all_generations(&scoped_key, storage::DeleteOptionsData::default())
            .await
            .map_err(|error| SessionManagementError::Storage(error.to_string()))?;
        let flat_outcome = self
            .dataset
            .delete_all_generations(&flat_key, storage::DeleteOptionsData::default())
            .await
            .map_err(|error| SessionManagementError::Storage(error.to_string()))?;
        if !outcome.deleted_primary() && !outcome.deleted_previous() {
            outcome = flat_outcome;
        }
        // #2163 P2：级联删除该 session 名下的 sub run 存档（`subruns/<id>/…`）。
        // 删除失败不静默吞掉：父 session 已删时返回错误，由调用方决定重试。
        self.delete_subruns_for(id).await?;
        let legacy_outcome = self.legacy.delete_for_project(id, project).await.err();
        if !outcome.deleted_primary() && !outcome.deleted_previous() && legacy_outcome.is_some() {
            return Err(SessionManagementError::NotFound(id.to_string()));
        }
        Ok(())
    }
}
// #2163（main）：sub run dataset 清理随 main 合入；sealed 已随实现权治理移除。
impl DatasetSessionManagement {
    /// 删除 `subruns/<parent_session_id>/` 下的全部 sub run dataset。
    ///
    /// key 形态：`[<project>, "subruns", <parent_session_id>, "<sub>.dataset"]`。
    /// 按段数（4）与第 3 段匹配父 session；幂等（无匹配即无操作）。
    async fn delete_subruns_for(
        &self,
        parent_session_id: &str,
    ) -> Result<(), SessionManagementError> {
        let dataset_keys = self
            .dataset
            .list_datasets(StorageNamespaceData::Session)
            .await
            .map_err(|error| SessionManagementError::Storage(error.to_string()))?;
        for dataset_key in dataset_keys {
            let segments = dataset_key.segments();
            if segments.len() != 4
                || segments[1].as_str() != "subruns"
                || segments[2].as_str() != parent_session_id
            {
                continue;
            }
            self.dataset
                .delete_all_generations(&dataset_key, storage::DeleteOptionsData::default())
                .await
                .map_err(|error| SessionManagementError::Storage(error.to_string()))?;
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "dataset_session_management_tests.rs"]
mod tests;
