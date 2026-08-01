use std::sync::{Arc, RwLock};

use async_trait::async_trait;

use crate::domain::{
    AcceptedInputAppend, AcceptedInputError, AcceptedInputReceipt, AppendReceipt, CompactOutcome,
    CompactRequest, ContextAppend, ContextAppendError, ContextMessages, ContextPortError,
    ContextRequest, ManualCompactRequest, SessionId, SessionRevision, SystemBlock,
    ToolReceiptMutation, ToolReceiptMutationError, ToolReceiptMutationReceipt,
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
    pub messages: ContextMessages,
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
    async fn advance_tool_receipt(
        &self,
        _mutation: ToolReceiptMutation,
    ) -> Result<ToolReceiptMutationReceipt, ToolReceiptMutationError> {
        Err(ToolReceiptMutationError::Storage(
            "此 SessionRepository 未实现 Tool receipt 持久化".to_string(),
        ))
    }
    async fn compare_and_record_skill_load(
        &self,
        _mutation: tools::SkillLoadMutation,
    ) -> Result<tools::SkillLoadDecision, tools::SkillLoadStateError> {
        Err(tools::SkillLoadStateError::Storage(
            "此 SessionRepository 未实现 Skill 加载状态持久化".to_string(),
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
/// 与 live Project `WorkspaceRead` 快照构造 `tools::SkillQuery`。
pub trait SkillQueryFactory: Send + Sync {
    fn query(&self, request: &ContextRequest) -> tools::SkillQuery;
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
