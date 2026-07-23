use async_trait::async_trait;

use share::session_types::ProjectIdentity;

use crate::domain::session::{
    CanonicalSession, SessionListEntry, SessionManagementError, SessionMetadataUpdate,
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
        project: &ProjectIdentity,
    ) -> Result<CanonicalSession, SessionManagementError>;

    /// Lists only sessions belonging to the supplied stable project identity.
    async fn list_for_project(
        &self,
        project: &ProjectIdentity,
    ) -> Result<Vec<SessionListEntry>, SessionManagementError>;

    /// Exports only a session belonging to the supplied stable project identity.
    async fn export_for_project(
        &self,
        id: &str,
        project: &ProjectIdentity,
    ) -> Result<Vec<u8>, SessionManagementError>;

    /// Imports only a session whose persisted project identity matches the
    /// supplied current project.
    async fn import_for_project(
        &self,
        bytes: &[u8],
        project: &ProjectIdentity,
    ) -> Result<SessionListEntry, SessionManagementError>;

    /// Updates metadata only for a session belonging to the supplied stable
    /// project identity.
    async fn update_metadata_for_project(
        &self,
        id: &str,
        project: &ProjectIdentity,
        update: SessionMetadataUpdate,
    ) -> Result<SessionListEntry, SessionManagementError>;

    /// Deletes only a session belonging to the supplied stable project identity.
    async fn delete_for_project(
        &self,
        id: &str,
        project: &ProjectIdentity,
    ) -> Result<(), SessionManagementError>;
}
