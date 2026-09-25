//! Audit：Usage Fact 的追加式存储与读出。
//!
//! # Published Language（#1705 收敛后，15 实体）
//!
//! | 类别 | 实体 | 消费者 |
//! |---|---|---|
//! | 装配 | `start_usage_worker`、`file_usage_append_store`、`UsageWorker`、`UsageSender`、`UsageWorkerConfig` | composition（装配根）；Config 另被 share snapshot 引用 |
//! | 数据契约 | `UsageEmitOutcome`、`UsageRecord`、`UsageSummary`、`UsageDropReason` | composition、runtime（发送分支/事实记录）；Summary 被 cli TUI 用量展示消费 |
//! | 读出（未接线） | `usage_query_service`、`UsageQuery`、`UsagePage`、`Pagination`、`TimeRange` | 暂无生产消费者（集成测试流程探针）；TUI 用量页接线时评审 |
//! | 边界错误 | `AuditError`（crate 根定义，`#[non_exhaustive]`） | 消费方只 match 粗分类；内部变体经 `From` 折叠不越界 |
//!
//! 边界约定：查询/Append 内部面（Port trait、schema 版本、Envelope、AppendLog 细节）
//! 为 crate 私有；错误细粒度原因保留在本 crate 日志，跨界仅传
//! `share::error::ErrorCategory` 对齐的粗分类。

/// Audit 模块自身的运行诊断 target；Audit Usage Fact 使用独立 append store。
pub(crate) const LOG_TARGET: &str = "aemeath:diagnostic:audit";

mod adapters;
mod application;
pub(crate) mod contract;
mod domain;
mod ports;

/// Audit 边界错误：对外粗分类（与 `share::error::ErrorCategory` 对齐）；
/// 内部错误（domain/ports 细节）经 `From` 折叠，细粒度原因留在 crate 内日志。
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuditError {
    Storage(String),
    Invalid(String),
    Unavailable(String),
}

impl std::fmt::Display for AuditError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuditError::Storage(message) => write!(formatter, "storage: {message}"),
            AuditError::Invalid(message) => write!(formatter, "invalid: {message}"),
            AuditError::Unavailable(message) => write!(formatter, "unavailable: {message}"),
        }
    }
}

impl AuditError {
    /// 与 `share::error::ErrorCategory` 对齐的粗分类。
    pub fn category(&self) -> share::error::ErrorCategory {
        match self {
            AuditError::Storage(_) => share::error::ErrorCategory::Storage,
            AuditError::Invalid(_) => share::error::ErrorCategory::Invalid,
            AuditError::Unavailable(_) => share::error::ErrorCategory::Unavailable,
        }
    }
}

impl From<crate::domain::UsageQueryError> for AuditError {
    fn from(inner: crate::domain::UsageQueryError) -> Self {
        match inner {
            crate::domain::UsageQueryError::Storage(message) => AuditError::Storage(message),
            crate::domain::UsageQueryError::InvalidRange => {
                AuditError::Invalid("非法查询区间".to_owned())
            }
            crate::domain::UsageQueryError::InvalidCursor => {
                AuditError::Invalid("非法分页游标".to_owned())
            }
        }
    }
}

impl From<crate::ports::AppendLogError> for AuditError {
    fn from(inner: crate::ports::AppendLogError) -> Self {
        AuditError::Storage(inner.to_string())
    }
}

/// Published Language：composition 装配面（worker 管道）、cli 展示 DTO、
/// 读出面（集成测试流程探针）与边界错误。
///
/// 查询/Append 内部面未接线（无生产消费者），按消费者证明制收窄 crate 内；
/// 契约测试已迁 crate 内单元测试（#1705）。
pub use adapters::{file_usage_append_store, usage_query_service};
pub use application::{start_usage_worker, UsageSender, UsageWorker, UsageWorkerConfig};
pub use domain::{
    Pagination, TimeRange, UsageDropReason, UsageEmitOutcome, UsagePage, UsageQuery, UsageRecord,
    UsageSummary,
};
