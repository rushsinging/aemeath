use std::sync::{Arc, RwLock};

use async_trait::async_trait;

use crate::domain::{
    AcceptedInputAppendData, AcceptedInputError, AcceptedInputReceiptData, AppendReceiptData,
    CompactOutcome, CompactRequestData, ContextAppendData, ContextAppendError, ContextMessages,
    ContextPortError, ContextRequestData, ManualCompactRequestData, SessionId, SessionRevision,
    SystemBlock, ToolReceiptMutationData, ToolReceiptMutationError, ToolReceiptMutationReceiptData,
};

pub mod context_port;
pub mod session_management;
pub mod session_snapshot_store;
pub(crate) use crate::domain::PromptMaterializationError;
pub use context_port::ContextPort;
pub use session_management::SessionManagementPort;
pub(crate) use session_snapshot_store::{
    SessionGeneration, SessionSnapshotStore, SessionStoreError,
};

pub trait MainContextFactory: Send + Sync {
    fn build(
        &self,
        session: Arc<RwLock<Arc<crate::domain::session::CanonicalSession>>>,
        task_persist: Arc<dyn task::TaskPersist>,
        workspace_persist: Arc<dyn project::WorkspaceWriter>,
        memory: Arc<RwLock<Arc<dyn memory::api::MemoryPort>>>,
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
pub(crate) struct SessionSnapshot {
    pub revision: SessionRevision,
    pub messages: ContextMessages,
    pub structured_history: Option<crate::domain::session::SessionHistory>,
    pub active_summary: Option<String>,
}

#[async_trait]
pub(crate) trait SessionRepository: Send + Sync {
    async fn snapshot(&self, session_id: &SessionId) -> Result<SessionSnapshot, String>;
    async fn append_accepted_input(
        &self,
        _append: &AcceptedInputAppendData,
    ) -> Result<AcceptedInputReceiptData, AcceptedInputError> {
        Err(AcceptedInputError::Storage(
            "此 SessionRepository 未实现已接受输入持久化".to_string(),
        ))
    }
    async fn advance_tool_receipt(
        &self,
        _mutation: ToolReceiptMutationData,
    ) -> Result<ToolReceiptMutationReceiptData, ToolReceiptMutationError> {
        Err(ToolReceiptMutationError::Storage(
            "此 SessionRepository 未实现 Tool receipt 持久化".to_string(),
        ))
    }
    async fn step_receipts(
        &self,
        _session_id: &SessionId,
        _run_id: &sdk::RunId,
        _step_id: &sdk::RunStepId,
    ) -> Result<Vec<crate::domain::StepReceiptData>, ToolReceiptMutationError> {
        Err(ToolReceiptMutationError::Storage(
            "此 SessionRepository 未实现 Step receipt 查询".to_string(),
        ))
    }
    async fn compare_and_record_skill_load(
        &self,
        _mutation: tools::published::skill::SkillLoadMutation,
    ) -> Result<
        tools::published::skill::SkillLoadDecision,
        tools::published::skill::SkillLoadStateError,
    > {
        Err(tools::published::skill::SkillLoadStateError::Storage(
            "此 SessionRepository 未实现 Skill 加载状态持久化".to_string(),
        ))
    }
    async fn append_finalized(
        &self,
        append: &ContextAppendData,
    ) -> Result<AppendReceiptData, ContextAppendError>;
    async fn commit_compaction(
        &self,
        request: &CompactRequestData,
    ) -> Result<CompactOutcome, ContextPortError>;
    async fn commit_manual_compaction(
        &self,
        request: &ManualCompactRequestData,
    ) -> Result<CompactOutcome, ContextPortError>;
    async fn clear(&self, session_id: &SessionId) -> Result<(), ContextPortError>;
}

#[derive(Debug, Clone)]
pub(crate) struct PromptMaterialization {
    pub cacheable: Vec<SystemBlock>,
    pub uncached: Vec<SystemBlock>,
}

/// Context-owned 查询工厂：为每次 `materialize(request)` 从 request/config
/// 与 live Project `WorkspaceReader` 快照构造 `tools::published::skill::SkillQuery`。
pub trait SkillQueryFactory: Send + Sync {
    fn query(&self, request: &ContextRequestData) -> tools::published::skill::SkillQuery;
}

#[async_trait]
pub(crate) trait ContextPromptSource: Send + Sync {
    async fn materialize(
        &self,
        request: &ContextRequestData,
    ) -> Result<PromptMaterialization, PromptMaterializationError>;
}

#[derive(Debug, Clone)]
pub(crate) struct MemoryMaterialization {
    pub blocks: Vec<SystemBlock>,
    /// 内容派生 revision（记忆注入结果变更检测；单元测试断言其随命中内容变化，
    /// 生产读侧待接线）。
    #[cfg_attr(not(test), allow(dead_code))]
    pub revision: u64,
}

#[async_trait]
pub(crate) trait ContextMemorySource: Send + Sync {
    async fn materialize(
        &self,
        request: &ContextRequestData,
    ) -> Result<MemoryMaterialization, String>;
}
