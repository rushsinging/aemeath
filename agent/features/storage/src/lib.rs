//! Storage：原子 blob / dataset 的文件系统实现与安全路径访问。
//!
//! # Published Language（四类语法，#1714 收敛）
//!
//! | 组 | 实体 |
//! |---|---|
//! | 工厂 | `wire_file_system_blob`、`wire_file_system_dataset`（原 file_system_* 更名 wire_ 前缀类；返回 `Arc<dyn 端口>`） |
//! | 角色和职能 | `AtomicBlobPort`（追加日志端口）、`AtomicDatasetPort`（原子集端口）、`SafeStorageRoot`/`SafeStorageDir`/`SafeOpenOptions`/`SafeStorageFileType`（安全文件系统句柄） |
//! | 数据和生命周期 | 28 个 Data：标识 7（StorageKeyData/DatasetKeyData 同构但语义域不同——类型安全优先不合）、载荷 9（含 SafeStorageEntryData 泄漏补导出）、回执 10、选项 3 |
//! | Error | `StorageError`{kind,message}（事实形态，**判定保留不折叠**——memory/config 穷尽 match kind 的控制流依赖 + kind 为 storage 专有语义域）、`StorageErrorKind`（6 变体）、`CorruptTransactionError`（+`CorruptionReason`/`QuarantineDisposition` 载荷链）——Error 类不 Data 化 |
//!
//! 判定记录：`PreviousPolicy` 降 pub(crate)（跨 crate 零消费但内部删除逻辑活跃）；
//! `QuarantineOutcomeData::AlreadyAbsent` 与 receipt 三字段重复但内嵌收益小于嵌套成本，保留；
//! 按 docs/design/03-engineering/05-published-language.md SOP。

/// 本 crate 的日志 target。所有 log::xxx! 调用必须引用此常量。
mod constants;
pub(crate) use constants::LOG_TARGET;

mod adapters;
mod domain;
mod ports;

use adapters::{FileSystemBlobAdapter, FileSystemDatasetAdapter};
use std::sync::Arc;

/// Composition wiring 唯一 blob 构造入口：concrete adapter 不进入稳定公开面。
pub fn wire_file_system_blob(
    root: impl AsRef<std::path::Path>,
) -> Result<Arc<dyn AtomicBlobPort>, StorageError> {
    log::debug!(target: crate::LOG_TARGET, "wire_file_system_blob init enter");
    match FileSystemBlobAdapter::new(root) {
        Ok(adapter) => {
            log::info!(target: crate::LOG_TARGET, "wire_file_system_blob init ok");
            Ok(Arc::new(adapter))
        }
        Err(error) => {
            log::error!(target: crate::LOG_TARGET, "wire_file_system_blob init failed");
            Err(error)
        }
    }
}

/// Composition wiring 唯一 dataset 构造入口：concrete adapter 不进入稳定公开面。
pub fn wire_file_system_dataset(
    root: impl AsRef<std::path::Path>,
) -> Result<Arc<dyn AtomicDatasetPort>, StorageError> {
    log::debug!(target: crate::LOG_TARGET, "wire_file_system_dataset init enter");
    match FileSystemDatasetAdapter::new(root) {
        Ok(adapter) => {
            log::info!(target: crate::LOG_TARGET, "wire_file_system_dataset init ok");
            Ok(Arc::new(adapter))
        }
        Err(error) => {
            log::error!(target: crate::LOG_TARGET, "wire_file_system_dataset init failed");
            Err(error)
        }
    }
}

pub use adapters::{
    SafeOpenOptions, SafeStorageDir, SafeStorageEntryData, SafeStorageFileType, SafeStorageRoot,
};
pub use domain::{
    BlobReadData, CommitWarningData, CorruptTransactionError, CorruptionReason,
    DatasetChangeSetData, DatasetCommitReceiptData, DatasetCommitVisibilityData, DatasetKeyData,
    DatasetManifestData, DatasetMemberChangeData, DatasetMemberData, DatasetMemberReferenceData,
    DatasetReadData, DatasetReadOutcomeData, DatasetRevisionData, DeleteOptionsData,
    DeleteOutcomeData, DurabilityData, GenerationData, PromoteOutcomeData, QuarantineDisposition,
    QuarantineOutcomeData, QuarantineReason, QuarantineReceiptData, ReadOutcomeData,
    SafePathSegmentData, StorageEntryData, StorageError, StorageErrorKind, StorageKeyData,
    StorageNamespaceData, TransactionScopeData, WriteOptionsData, WriteReceiptData,
};
pub use ports::{AtomicBlobPort, AtomicDatasetPort};

#[cfg(test)]
#[path = "test_log.rs"]
pub(crate) mod test_log;

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
