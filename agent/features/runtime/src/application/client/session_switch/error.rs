//! Main Session resume / binding 的结构化错误。
//!
//! 平移自 context `application/main_session.rs`（#2025）：每个变体标识
//! 跨 BC prepare/commit 流水线的一个阶段，调用方可据此区分 resume 被拒原因。

use memory::api::{MemoryOpenError, MemoryOpenerError};
use share::error::DomainError;

#[derive(Debug, thiserror::Error)]
pub enum MainSessionError {
    /// 会话 envelope 的 `workspace` 槽位 `Missing` 或 `CapturedEmpty`。
    /// resume 必须有类型化的 workspace 上下文，没有安全默认值。
    #[error("workspace snapshot is missing or captured empty; a typed workspace context is required for resume")]
    WorkspaceMissing,

    /// 会话属于另一个稳定项目身份，禁止跨项目 resume。
    #[error("session belongs to a different project")]
    ProjectMismatch,

    /// `WorkspaceWriter::prepare_restore` 拒绝候选。
    #[error("workspace restore prepare failed: {0}")]
    WorkspaceRestore(#[from] DomainError),

    /// 推导 canonical project-config 位置失败。
    #[error("invalid config location: {0:?}")]
    ConfigLocation(DomainError),

    /// `ProjectConfigParticipant::prepare_for_project` 失败。
    #[error("config prepare failed: {0:?}")]
    ConfigPrepare(DomainError),

    /// 推导 project memory key 失败。
    #[error("memory key derivation failed: {0}")]
    MemoryKey(#[from] MemoryOpenError),

    /// `MemoryOpener::open_memory` 失败。
    #[error("memory open failed: {0}")]
    MemoryOpen(#[from] MemoryOpenerError),

    /// TaskData 侧 `TaskPersist::prepare_restore` 拒绝 task snapshot。
    #[error("task restore prepare failed: {0}")]
    TaskRestore(DomainError),

    /// 切换门禁已关闭（内部锁被丢弃）。
    #[error("session switch gate closed")]
    GateClosed,
}
