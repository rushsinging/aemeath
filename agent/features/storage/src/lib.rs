//! Storage：原子 blob / dataset 的文件系统实现与安全路径访问。
//!
//! # Published Language（四类语法，#1714 收敛）
//!
//! | 组 | 实体 |
//! |---|---|
//! | 工厂 | `wire_file_system_blob`、`wire_file_system_dataset`（原 file_system_* 更名 wire_ 前缀类；返回 `Arc<dyn 端口>`） |
//! | 角色和职能 | `AtomicBlobPort`（追加日志端口）、`AtomicDatasetPort`（原子集端口）、`SafeStorageRoot`/`SafeStorageDir`/`SafeOpenOptions`/`SafeStorageFileType`（安全文件系统句柄） |
//! | 数据和生命周期 | 29 个 Data：标识 7（StorageKeyData/DatasetKeyData 同构但语义域不同——类型安全优先不合）、载荷 9（含 SafeStorageEntryData 泄漏补导出）、回执 10、选项 3（#1865 E：原自述 28 实数 29 已校正） |
//! | Error | **无 Error 类根导出**（#1865 C 语义折叠 + 零消费收窄）：跨界统一 `share::error::DomainError`——内部 `StorageError`/`StorageErrorKind` 与损坏载荷 `CorruptTransactionError`/`CorruptionReason`/`QuarantineDisposition` 全部 pub(crate)，`impl From<StorageError>` 为唯一折叠点（InvalidKey→Invalid / UnsupportedDurability→Unavailable / 其余→Storage） |
//!
//! #1865 收敛记录：A 恢复决策单点（`domain::blob_recovery` 为唯一策略表，adapter 只采集
//! 证据并执行决策；JournalPhase 单点带 serde，TransactionDigest 删除）；B 死面清理
//! （Namespace 4 死变体 + PreviousPolicy 机制、`CommitWarningData::PreviousPromotionPending`
//! /`MemberPublishRecoveryPending`、`DatasetCommitReceiptData.warning` 字段、
//! `QuarantineReceiptData::id`、`StorageEntryData::generation` 全部删除；`DatasetReadData`
//! 保留——方法访问盲区，实为 read 载荷承载体）。
//! `QuarantineOutcomeData::AlreadyAbsent` 与 receipt 三字段重复但内嵌收益小于嵌套成本，保留；
//! 按 docs/design/03-engineering/05-published-language.md SOP。

/// 本 crate 的日志 target。所有 log::xxx! 调用必须引用此常量。
mod constants;
pub(crate) use constants::LOG_TARGET;
/// #1865 C：事实错误降 pub(crate)（折叠为 DomainError 后不再根导出），
/// crate 内旧路径保持可用。
pub(crate) use domain::{StorageError, StorageErrorKind};

mod adapters;
mod domain;
mod ports;

use adapters::{FileSystemBlobAdapter, FileSystemDatasetAdapter};
use std::sync::Arc;

/// Composition wiring 唯一 blob 构造入口：concrete adapter 不进入稳定公开面。
pub fn wire_file_system_blob(
    root: impl AsRef<std::path::Path>,
) -> Result<Arc<dyn AtomicBlobPort>, share::error::DomainError> {
    log::debug!(target: crate::LOG_TARGET, "wire_file_system_blob init enter");
    match FileSystemBlobAdapter::new(root) {
        Ok(adapter) => {
            log::info!(target: crate::LOG_TARGET, "wire_file_system_blob init ok");
            Ok(Arc::new(adapter))
        }
        Err(error) => {
            log::error!(target: crate::LOG_TARGET, "wire_file_system_blob init failed");
            Err(share::error::DomainError::from(error))
        }
    }
}

/// Composition wiring 唯一 dataset 构造入口：concrete adapter 不进入稳定公开面。
pub fn wire_file_system_dataset(
    root: impl AsRef<std::path::Path>,
) -> Result<Arc<dyn AtomicDatasetPort>, share::error::DomainError> {
    log::debug!(target: crate::LOG_TARGET, "wire_file_system_dataset init enter");
    match FileSystemDatasetAdapter::new(root) {
        Ok(adapter) => {
            log::info!(target: crate::LOG_TARGET, "wire_file_system_dataset init ok");
            Ok(Arc::new(adapter))
        }
        Err(error) => {
            log::error!(target: crate::LOG_TARGET, "wire_file_system_dataset init failed");
            Err(share::error::DomainError::from(error))
        }
    }
}

pub use adapters::{
    SafeOpenOptions, SafeStorageDir, SafeStorageEntryData, SafeStorageFileType, SafeStorageRoot,
};
pub use domain::{
    BlobReadData, CommitWarningData, DatasetChangeSetData, DatasetCommitReceiptData,
    DatasetCommitVisibilityData, DatasetKeyData, DatasetManifestData, DatasetMemberChangeData,
    DatasetMemberData, DatasetMemberReferenceData, DatasetReadData, DatasetReadOutcomeData,
    DatasetRevisionData, DeleteOptionsData, DeleteOutcomeData, DurabilityData, GenerationData,
    PromoteOutcomeData, QuarantineOutcomeData, QuarantineReason, QuarantineReceiptData,
    ReadOutcomeData, SafePathSegmentData, StorageEntryData, StorageKeyData, StorageNamespaceData,
    TransactionScopeData, WriteOptionsData, WriteReceiptData,
};
pub use ports::{AtomicBlobPort, AtomicDatasetPort};

#[cfg(test)]
#[path = "test_log.rs"]
pub(crate) mod test_log;

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
