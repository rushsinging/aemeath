//! ContextPort — Context Management 对 Runtime 开放的 OHS 端口。
//!
//! Runtime 经 3 个方法读写上下文，不接触 Session / ChatChain / compact 管线内部。
//!
//! 设计文档：`docs/design/02-modules/context-management/README.md` §5
//!
//! **注意**：本 trait 目前是骨架定义（Batch 1）。Batch 2-4 迁移 session/compact
//! 后将补全具体签名与实现。

use async_trait::async_trait;

/// Context Management 对 Agent Runtime 开放的端口。
///
/// Runtime 的每个 RunStep：
/// - 开始时调 [`build_window`] 构建本轮 Context Window；
/// - 结束时调 [`append_and_persist`] 追加对话并落盘；
/// - 在 LLM 调用前调 [`needs_compaction`] 判断是否需要压缩。
///
/// Runtime **NEVER** 直接接触 Session、ChatChain 或 compact 管线内部结构。
#[async_trait]
pub trait ContextPort: Send + Sync {
    /// 构建本轮 Context Window。
    ///
    /// 内部步骤：L1-L4 compact → memory 注入 → prompt 组装 → 返回 messages。
    ///
    /// 返回序列化后的 messages JSON（经 ACL 隔离，不泄漏 provider 线格式）。
    async fn build_window(
        &self,
        session_id: &str,
    ) -> Result<Vec<serde_json::Value>, ContextPortError>;

    /// 是否需要压缩。
    ///
    /// 内部步骤：token budget 计算 → 返回 compaction urgency。
    fn needs_compaction(&self, session_id: &str) -> CompactionUrgency;

    /// 追加对话并落盘。
    ///
    /// 内部步骤：写入 ChatChain → 收集跨 BC 快照 → 原子落盘。
    async fn append_and_persist(
        &self,
        session_id: &str,
        messages: &[serde_json::Value],
    ) -> Result<(), ContextPortError>;
}

/// Compact 紧迫程度。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompactionUrgency {
    /// 无需压缩。
    None,
    /// 建议压缩（接近阈值）。
    Soon,
    /// 必须压缩（已达阈值）。
    Urgent,
}

/// ContextPort 错误。
#[derive(Debug, Clone, thiserror::Error)]
pub enum ContextPortError {
    #[error("session not found: {0}")]
    SessionNotFound(String),
    #[error("storage error: {0}")]
    Storage(String),
    #[error("compact error: {0}")]
    Compact(String),
}
