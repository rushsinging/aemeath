use async_trait::async_trait;

use share::session_types::ProjectIdentityData;

use crate::domain::session::{
    CanonicalSession, DisplayHistoryStepWindow, SessionListEntry, SessionManagementError,
    SessionMetadataUpdate, SessionResumeLoad,
};

/// Context-owned Session identity management contract.
///
/// Composition supplies one implementation to both `MainSessionWiring` and
/// Runtime. Consumers never select Storage adapters or inspect blob protocol files.
#[async_trait]
pub trait SessionManagementPort: Send + Sync {
    /// Loads only a session whose persisted workspace has the current stable
    /// project identity. Git worktrees match by common-dir, non-git by root.
    async fn load_for_project(
        &self,
        id: &str,
        project: &ProjectIdentityData,
    ) -> Result<CanonicalSession, SessionManagementError>;

    async fn load_for_resume(
        &self,
        id: &str,
        project: &ProjectIdentityData,
    ) -> Result<SessionResumeLoad, SessionManagementError> {
        self.load_for_project(id, project)
            .await
            .map(|active_session| SessionResumeLoad {
                active_session,
                display_history: None,
            })
    }

    async fn load_display_history_steps(
        &self,
        id: &str,
        project: &ProjectIdentityData,
        generation_revision: u64,
        member_names: &[String],
    ) -> Result<DisplayHistoryStepWindow, SessionManagementError> {
        let _ = (id, project, generation_revision, member_names);
        Err(SessionManagementError::Storage(
            "当前 Session 存储不支持按需 display history".to_string(),
        ))
    }

    /// Lists only sessions belonging to the supplied stable project identity.
    async fn list_for_project(
        &self,
        project: &ProjectIdentityData,
    ) -> Result<Vec<SessionListEntry>, SessionManagementError>;

    /// Exports only a session belonging to the supplied stable project identity.
    async fn export_for_project(
        &self,
        id: &str,
        project: &ProjectIdentityData,
    ) -> Result<Vec<u8>, SessionManagementError>;

    /// Imports only a session whose persisted project identity matches the
    /// supplied current project.
    async fn import_for_project(
        &self,
        bytes: &[u8],
        project: &ProjectIdentityData,
    ) -> Result<SessionListEntry, SessionManagementError>;

    /// Updates metadata only for a session belonging to the supplied stable
    /// project identity.
    async fn update_metadata_for_project(
        &self,
        id: &str,
        project: &ProjectIdentityData,
        update: SessionMetadataUpdate,
    ) -> Result<SessionListEntry, SessionManagementError>;

    /// Deletes only a session belonging to the supplied stable project identity.
    async fn delete_for_project(
        &self,
        id: &str,
        project: &ProjectIdentityData,
    ) -> Result<(), SessionManagementError>;
}
