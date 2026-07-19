use std::sync::{Arc, RwLock};

use async_trait::async_trait;

use crate::domain::{
    AppendReceipt, CompactOutcome, CompactRequest, ContextAppend, ContextAppendError,
    ContextMessage, ContextPortError, ContextRequest, SessionId, SessionRevision, SystemBlock,
};

pub mod context_port;
pub mod session_snapshot_store;
pub use context_port::ContextPort;
pub use session_snapshot_store::{SessionGeneration, SessionSnapshotStore, SessionStoreError};

pub trait MainContextFactory: Send + Sync {
    fn build(
        &self,
        session: Arc<RwLock<Arc<crate::domain::session::CanonicalSession>>>,
        task_persist: Arc<dyn task::TaskPersist>,
        workspace_persist: Arc<dyn project::WorkspacePersist>,
        memory: Arc<RwLock<Arc<dyn memory::MemoryPort>>>,
        mutation_gate: Arc<tokio::sync::Mutex<()>>,
    ) -> Arc<dyn ContextPort>;
}

pub trait SessionDecoder: Send + Sync {
    fn decode(
        &self,
        bytes: &[u8],
    ) -> Result<crate::domain::session::DecodedSession, crate::domain::session::SessionCodecError>;
}

#[derive(Debug, Clone)]
pub struct SessionSnapshot {
    pub revision: SessionRevision,
    pub messages: Vec<ContextMessage>,
    pub active_summary: Option<String>,
}

#[async_trait]
pub trait SessionRepository: Send + Sync {
    async fn snapshot(&self, session_id: &SessionId) -> Result<SessionSnapshot, String>;
    async fn append_finalized(
        &self,
        append: &ContextAppend,
    ) -> Result<AppendReceipt, ContextAppendError>;
    async fn commit_compaction(
        &self,
        request: &CompactRequest,
    ) -> Result<CompactOutcome, ContextPortError>;
}

#[derive(Debug, Clone)]
pub struct PromptMaterialization {
    pub cacheable: Vec<SystemBlock>,
    pub uncached: Vec<SystemBlock>,
    pub revision: u64,
}

#[async_trait]
pub trait ContextPromptSource: Send + Sync {
    async fn materialize(&self, request: &ContextRequest) -> Result<PromptMaterialization, String>;
}

#[derive(Debug, Clone)]
pub struct MemoryMaterialization {
    pub blocks: Vec<SystemBlock>,
    pub revision: u64,
}

#[async_trait]
pub trait ContextMemorySource: Send + Sync {
    async fn materialize(&self, request: &ContextRequest) -> Result<MemoryMaterialization, String>;
}
