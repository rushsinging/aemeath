//! ContextPort — Context Management 对 Runtime 发布的类型化 OHS。

use async_trait::async_trait;

pub use crate::domain::*;

/// Context Management 对 Agent Runtime 开放的唯一端口。
///
/// Runtime 每个 RunStep 开始时构建 window；需要时执行 compact；普通完成、
/// CancelRunStep 或 TerminateRun 经 StepFinalizer 收口后提交唯一 ContextAppend。
#[async_trait]
pub trait ContextPort: Send + Sync {
    async fn build_window(
        &self,
        request: &ContextRequest,
    ) -> Result<ContextWindow, ContextPortError>;

    async fn needs_compaction(
        &self,
        request: &ContextRequest,
    ) -> Result<CompactionDecision, ContextPortError>;

    async fn compact(&self, request: &CompactRequest) -> Result<CompactOutcome, ContextPortError>;

    async fn manual_compact(
        &self,
        request: &ManualCompactRequest,
    ) -> Result<CompactOutcome, ContextPortError>;

    async fn clear_session(&self, session_id: &SessionId) -> Result<(), ContextPortError>;

    async fn append_accepted_input(
        &self,
        _append: &AcceptedInputAppend,
    ) -> Result<AcceptedInputReceipt, AcceptedInputError> {
        Err(AcceptedInputError::Storage(
            "此 ContextPort 未实现已接受输入持久化".to_string(),
        ))
    }

    async fn append_and_persist(
        &self,
        append: &ContextAppend,
    ) -> Result<AppendReceipt, ContextAppendError>;
}
