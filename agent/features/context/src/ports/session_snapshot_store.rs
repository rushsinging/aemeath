use async_trait::async_trait;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionGeneration {
    Primary,
    Previous,
}

#[derive(Debug, thiserror::Error)]
#[error("Session snapshot store failed: {0}")]
pub struct SessionStoreError(pub String);

#[async_trait]
pub trait SessionSnapshotStore: Send + Sync {
    async fn read(
        &self,
        generation: SessionGeneration,
    ) -> Result<Option<Vec<u8>>, SessionStoreError>;
    async fn write(&self, bytes: &[u8]) -> Result<(), SessionStoreError>;
    async fn promote_previous(&self) -> Result<(), SessionStoreError>;
    async fn quarantine(&self, generation: SessionGeneration) -> Result<(), SessionStoreError>;
}
