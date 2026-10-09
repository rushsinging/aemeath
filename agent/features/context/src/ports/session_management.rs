use async_trait::async_trait;

use share::session_types::ProjectIdentityData;

use share::error::DomainError;

use crate::domain::session::{
    CanonicalSession, DisplayHistoryStepWindowData, SessionListEntryData,
    SessionMetadataUpdateData, SessionResumeLoad,
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
    ) -> Result<CanonicalSession, DomainError>;

    async fn load_for_resume(
        &self,
        id: &str,
        project: &ProjectIdentityData,
    ) -> Result<SessionResumeLoad, DomainError> {
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
    ) -> Result<DisplayHistoryStepWindowData, DomainError> {
        let _ = (id, project, generation_revision, member_names);
        Err(DomainError::storage(
            "context",
            "Session 存储失败：当前 Session 存储不支持按需 display history",
        )
        .with_code(crate::error_codes::SESSION_STORAGE))
    }

    /// Lists only sessions belonging to the supplied stable project identity.
    async fn list_for_project(
        &self,
        project: &ProjectIdentityData,
    ) -> Result<Vec<SessionListEntryData>, DomainError>;

    /// Exports only a session belonging to the supplied stable project identity.
    async fn export_for_project(
        &self,
        id: &str,
        project: &ProjectIdentityData,
    ) -> Result<Vec<u8>, DomainError>;

    /// Imports only a session whose persisted project identity matches the
    /// supplied current project.
    async fn import_for_project(
        &self,
        bytes: &[u8],
        project: &ProjectIdentityData,
    ) -> Result<SessionListEntryData, DomainError>;

    /// Updates metadata only for a session belonging to the supplied stable
    /// project identity.
    async fn update_metadata_for_project(
        &self,
        id: &str,
        project: &ProjectIdentityData,
        update: SessionMetadataUpdateData,
    ) -> Result<SessionListEntryData, DomainError>;

    /// Deletes only a session belonging to the supplied stable project identity.
    async fn delete_for_project(
        &self,
        id: &str,
        project: &ProjectIdentityData,
    ) -> Result<(), DomainError>;
}
