use std::sync::Arc;

use async_trait::async_trait;
use storage::{AtomicBlobPort, SafePathSegment, StorageNamespace};

use crate::adapters::{AtomicBlobSessionStore, LegacySessionDecoder};
use crate::application::{SessionLoadError, SessionPersistenceService};
use crate::domain::session::{
    now_iso, project_dir_segment, session_matches_project, CanonicalSession, SessionCodec,
    SessionListEntry, SessionManagementError, SessionMetadataUpdate,
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

    fn store_scoped(
        &self,
        project_dir: &SafePathSegment,
        id: &str,
    ) -> Result<Arc<AtomicBlobSessionStore>, SessionManagementError> {
        AtomicBlobSessionStore::new_scoped(Arc::clone(&self.blob), project_dir, id)
            .map(Arc::new)
            .map_err(|error| SessionManagementError::Storage(error.to_string()))
    }

    fn persistence_from_store(
        &self,
        store: Arc<AtomicBlobSessionStore>,
    ) -> SessionPersistenceService {
        SessionPersistenceService::new(store, Arc::new(LegacySessionDecoder))
    }

    /// 两级加载：scoped key（迁移后）→ 平铺 key（迁移前）→ 平铺带 `.json`
    /// 后缀（更早版本布局）。逐候选探测，第一个成功者胜出。
    async fn load_canonical(
        &self,
        project_dir: Option<&SafePathSegment>,
        id: &str,
    ) -> Result<CanonicalSession, SessionManagementError> {
        let mut candidates: Vec<Arc<AtomicBlobSessionStore>> = Vec::new();
        if let Some(project_dir) = project_dir {
            candidates.push(self.store_scoped(project_dir, id)?);
        }
        candidates.push(self.store(id)?);
        if let Ok(store) = AtomicBlobSessionStore::from_key_segments(
            Arc::clone(&self.blob),
            vec![format!("{id}.json")],
        ) {
            candidates.push(Arc::new(store));
        }
        let mut last_error: Option<SessionLoadError> = None;
        for store in candidates {
            let mut session = match self.persistence_from_store(Arc::clone(&store)).load().await {
                Ok(session) => session,
                Err(error) => {
                    last_error = Some(error);
                    continue;
                }
            };
            crate::adapters::tool_receipt_ledger::AtomicBlobToolReceiptLedger::new(
                Arc::clone(&self.blob),
                id,
            )
            .map_err(SessionManagementError::Storage)?
            .overlay(&mut session)
            .await
            .map_err(SessionManagementError::Storage)?;
            return Ok(session);
        }
        Err(match last_error {
            Some(error) => map_load(id, error),
            None => SessionManagementError::Storage("候选 session 存储构造失败".to_string()),
        })
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

    /// 按段过滤：首段为本项目目录段的 key 直接加载（仍做 identity 复核防
    /// 手放/哈希碰撞）；平铺单段 key（含 `.json` 后缀遗留）加载后按
    /// identity 过滤。跨项目 key 零加载。
    async fn list_for_project(
        &self,
        project: &share::session_types::ProjectIdentityData,
    ) -> Result<Vec<SessionListEntry>, SessionManagementError> {
        let project_dir = project_dir_segment(project);
        let entries = self
            .blob
            .list_primary(StorageNamespace::Session)
            .await
            .map_err(|error| SessionManagementError::Storage(error.to_string()))?;
        let mut sessions = Vec::new();
        for entry in entries {
            let segments = entry.key().segments();
            let (candidate_id, scoped) = match segments {
                [first, second] if *first == project_dir => (second.as_str().to_string(), true),
                [single] => (single.as_str().to_string(), false),
                _ => continue,
            };
            let session_id = candidate_id.strip_suffix(".json").unwrap_or(&candidate_id);
            if let Ok(session) = self.load_canonical(Some(&project_dir), session_id).await {
                if !scoped && !session_matches_project(&session, project) {
                    continue;
                }
                if scoped && !session_matches_project(&session, project) {
                    // 目录段与 identity 不符（手放/碰撞）：跳过。
                    continue;
                }
                sessions.push(SessionListEntry::from_canonical(&session));
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
        // 导入统一落新布局：session 自身 identity 决定目录段。
        let store = match crate::domain::session::session_project_dir(&session) {
            Some(project_dir) => self.store_scoped(&project_dir, &session.id)?,
            None => self.store(&session.id)?,
        };
        self.persistence_from_store(store)
            .save(&session)
            .await
            .map_err(|error| SessionManagementError::Storage(error.to_string()))?;
        Ok(SessionListEntry::from_canonical(&session))
    }

    async fn update_metadata_for_project(
        &self,
        id: &str,
        project: &share::session_types::ProjectIdentityData,
        update: SessionMetadataUpdate,
    ) -> Result<SessionListEntry, SessionManagementError> {
        let mut session = self.load_for_project(id, project).await?;
        update.apply(&mut session.metadata);
        session.updated_at = now_iso();
        // 元数据更新顺带把 session 升格到 project 目录段布局。
        let store = match crate::domain::session::session_project_dir(&session) {
            Some(project_dir) => self.store_scoped(&project_dir, id)?,
            None => self.store(id)?,
        };
        self.persistence_from_store(store)
            .save(&session)
            .await
            .map_err(|error| SessionManagementError::Storage(error.to_string()))?;
        Ok(SessionListEntry::from_canonical(&session))
    }

    async fn delete_for_project(
        &self,
        id: &str,
        project: &share::session_types::ProjectIdentityData,
    ) -> Result<(), SessionManagementError> {
        let session = self.load_for_project(id, project).await?;
        // 三种历史布局位置一并清理（scoped / 平铺 / 平铺 .json）。
        let mut stores = Vec::new();
        if let Some(project_dir) = crate::domain::session::session_project_dir(&session) {
            if let Ok(store) = self.store_scoped(&project_dir, id) {
                stores.push(store);
            }
        }
        stores.push(self.store(id)?);
        if let Ok(store) = AtomicBlobSessionStore::from_key_segments(
            Arc::clone(&self.blob),
            vec![format!("{id}.json")],
        ) {
            stores.push(Arc::new(store));
        }
        let mut deleted_any = false;
        for store in stores {
            let outcome = store
                .delete_all()
                .await
                .map_err(|error| SessionManagementError::Storage(error.to_string()))?;
            deleted_any |= outcome.deleted_primary() | outcome.deleted_previous();
        }
        crate::adapters::tool_receipt_ledger::AtomicBlobToolReceiptLedger::new(
            Arc::clone(&self.blob),
            id,
        )
        .map_err(SessionManagementError::Storage)?
        .delete()
        .await
        .map_err(SessionManagementError::Storage)?;
        if !deleted_any {
            return Err(SessionManagementError::NotFound(id.to_string()));
        }
        Ok(())
    }
}
