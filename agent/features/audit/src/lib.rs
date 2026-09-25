//! Audit：Usage Fact 的追加式存储与读出。
//!
//! # Published Language（四类封闭语法）
//!
//! | 类 | 实体 | 消费者 |
//! |---|---|---|
//! | `wire_*` 工厂 | `wire_audit_client`、`wire_audit_store` | composition |
//! | `*<Role>` 角色 | `AuditWriter`（try_record/shutdown，拥有 worker 管道）、`AuditReader`（query_page 纯读）、`AuditStore`（存储句柄，SPI 不出签名） | composition、runtime（经 UsageSink 适配）、TUI（Reader 接线待评审） |
//! | `*Data` 数据 | `UsageRecordData`、`UsageEmitOutcomeData`、`UsageDropReasonData`、`UsageSummaryData`、`UsageQueryData`、`UsagePageData`、`UsagePaginationData`、`UsageTimeRangeData` | composition、runtime、cli TUI（Summary） |
//! | `*Error` 错误 | `AuditError`（crate 根定义，粗分类） | query_page 签名 |
//!
//! Role 词表（v3，拟人/明确名词）：Reader/Writer/Control/Registry/Pool/Catalog/Store；
//! 数据一律 `Data` 尾缀；错误一律 `Error` 尾缀；工厂一律 `wire_` 前缀；
//! 读写配套由同一 wire 工厂产出（Writer+Reader），不造全能 Client。

/// Audit 模块自身的运行诊断 target；Audit Usage Fact 使用独立 append store。
pub(crate) const LOG_TARGET: &str = "aemeath:diagnostic:audit";

mod adapters;
mod application;
pub mod client;
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
use crate::adapters::append::file_usage_append_store;
pub use client::{wire_audit_client, wire_audit_store, AuditReader, AuditStore, AuditWriter};
/// 文件系统审计存储工厂（SPI 经 AuditStore 包装，此处返回 port 以便装配）。
pub fn append_store_for(
    root: storage::SafeStorageRoot,
) -> std::sync::Arc<dyn crate::ports::UsageAppendStorePort> {
    std::sync::Arc::new(file_usage_append_store(root))
}
pub use domain::{
    UsageDropReasonData, UsageEmitOutcomeData, UsagePageData, UsagePaginationData, UsageQueryData,
    UsageRecordData, UsageSummaryData, UsageTimeRangeData,
};
