use std::sync::{Arc, RwLock};

use async_trait::async_trait;

use crate::domain::{
    AcceptedInputAppend, AcceptedInputError, AcceptedInputReceipt, AppendReceipt, CompactOutcome,
    CompactRequest, ContextAppend, ContextAppendError, ContextMessage, ContextPortError,
    ContextRequest, ManualCompactRequest, SessionId, SessionRevision, SystemBlock,
};

pub mod context_port;
pub mod session_management;
pub mod session_snapshot_store;
pub use crate::domain::PromptMaterializationError;
pub use context_port::ContextPort;
pub use session_management::SessionManagementPort;
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
    async fn append_accepted_input(
        &self,
        _append: &AcceptedInputAppend,
    ) -> Result<AcceptedInputReceipt, AcceptedInputError> {
        Err(AcceptedInputError::Storage(
            "此 SessionRepository 未实现已接受输入持久化".to_string(),
        ))
    }
    async fn append_finalized(
        &self,
        append: &ContextAppend,
    ) -> Result<AppendReceipt, ContextAppendError>;
    async fn commit_compaction(
        &self,
        request: &CompactRequest,
    ) -> Result<CompactOutcome, ContextPortError>;
    async fn commit_manual_compaction(
        &self,
        request: &ManualCompactRequest,
    ) -> Result<CompactOutcome, ContextPortError>;
    async fn clear(&self, session_id: &SessionId) -> Result<(), ContextPortError>;
}

#[derive(Debug, Clone)]
pub struct PromptMaterialization {
    pub cacheable: Vec<SystemBlock>,
    pub uncached: Vec<SystemBlock>,
    pub revision: u64,
}

/// Context-owned 查询工厂：为每次 `materialize(request)` 从 request/config
/// 与 live Project `WorkspaceRead` 快照构造 `tools::SkillMaterializationQuery`，
/// 从而不捕获启动 cwd（worktree 切换后仍读取当前 workspace root）。
///
/// 生产实现持有 `Arc<dyn WorkspaceRead>`，每次调用读取
/// `current_workspace_root()`；测试可注入确定性 fake。
pub trait SkillQueryFactory: Send + Sync {
    /// 由 request/config 与注入的 workspace 快照推导物化查询。
    fn materialize_query(&self, request: &ContextRequest) -> tools::SkillMaterializationQuery;
}

#[async_trait]
pub trait ContextPromptSource: Send + Sync {
    async fn materialize(
        &self,
        request: &ContextRequest,
    ) -> Result<PromptMaterialization, PromptMaterializationError>;
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
