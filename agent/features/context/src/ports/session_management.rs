use async_trait::async_trait;

use crate::domain::session::{
    CanonicalSession, SessionListEntry, SessionManagementError, SessionMetadataUpdate,
};

/// Context-owned Session identity management contract.
///
/// Composition supplies one implementation to both `MainSessionWiring` and
/// Runtime. Consumers never select Storage adapters or inspect blob protocol files.
#[async_trait]
pub trait SessionManagementPort: Send + Sync {
    async fn load_canonical(&self, id: &str) -> Result<CanonicalSession, SessionManagementError>;

    async fn list(&self) -> Result<Vec<SessionListEntry>, SessionManagementError>;

    async fn export(&self, id: &str) -> Result<Vec<u8>, SessionManagementError>;

    async fn import(&self, bytes: &[u8]) -> Result<SessionListEntry, SessionManagementError>;

    async fn update_metadata(
        &self,
        id: &str,
        update: SessionMetadataUpdate,
    ) -> Result<SessionListEntry, SessionManagementError>;

    async fn delete(&self, id: &str) -> Result<(), SessionManagementError>;
}
