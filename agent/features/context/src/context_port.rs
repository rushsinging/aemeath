//! ContextPort — Context Management 对 Runtime 发布的类型化 OHS。

use async_trait::async_trait;

pub use crate::contract::*;

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

    async fn append_and_persist(
        &self,
        append: &ContextAppend,
    ) -> Result<AppendReceipt, ContextAppendError>;
}
