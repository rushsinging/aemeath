use async_trait::async_trait;

use crate::domain::{
    AppendReceipt, CompactOutcome, CompactRequest, ContextAppend, ContextAppendError,
    ContextMessage, ContextPortError, ContextRequest, SessionId, SessionRevision, SystemBlock,
};

pub mod context_port;
pub use context_port::ContextPort;

#[derive(Debug, Clone)]
pub struct SessionSnapshot {
    pub revision: SessionRevision,
    pub messages: Vec<ContextMessage>,
    pub active_summary: Option<String>,
}

#[async_trait]
pub trait SessionBacking: Send + Sync {
    async fn snapshot(&self, session_id: &SessionId) -> Result<SessionSnapshot, String>;
    async fn append(&self, append: &ContextAppend) -> Result<AppendReceipt, ContextAppendError>;
    async fn compact(&self, request: &CompactRequest) -> Result<CompactOutcome, ContextPortError>;
}

#[derive(Debug, Clone)]
pub struct WindowProjection {
    pub messages: Vec<ContextMessage>,
}

pub trait WindowProjector: Send + Sync {
    fn project(&self, messages: Vec<ContextMessage>) -> WindowProjection;
}

#[derive(Debug, Clone)]
pub struct PromptMaterialization {
    pub cacheable: Vec<SystemBlock>,
    pub uncached: Vec<SystemBlock>,
    pub revision: u64,
}

#[async_trait]
pub trait PromptMaterializer: Send + Sync {
    async fn materialize(&self, request: &ContextRequest) -> Result<PromptMaterialization, String>;
}

#[derive(Debug, Clone)]
pub struct MemoryMaterialization {
    pub blocks: Vec<SystemBlock>,
    pub revision: u64,
}

#[async_trait]
pub trait MemoryMaterializer: Send + Sync {
    async fn materialize(&self, request: &ContextRequest) -> Result<MemoryMaterialization, String>;
}
