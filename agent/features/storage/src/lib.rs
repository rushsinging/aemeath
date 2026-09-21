/// 本 crate 的日志 target。所有 log::xxx! 调用必须引用此常量。
pub(crate) const LOG_TARGET: &str = "aemeath:agent:storage";
mod adapters;
mod domain;
mod ports;

use adapters::{FileSystemBlobAdapter, FileSystemDatasetAdapter};
use std::sync::Arc;

/// Composition wiring 唯一 blob 构造入口：concrete adapter 不进入稳定公开面。
pub fn file_system_blob(
    root: impl AsRef<std::path::Path>,
) -> Result<Arc<dyn AtomicBlobPort>, StorageError> {
    log::debug!(target: crate::LOG_TARGET, "file_system_blob init enter");
    match FileSystemBlobAdapter::new(root) {
        Ok(adapter) => {
            log::info!(target: crate::LOG_TARGET, "file_system_blob init ok");
            Ok(Arc::new(adapter))
        }
        Err(error) => {
            log::error!(target: crate::LOG_TARGET, "file_system_blob init failed");
            Err(error)
        }
    }
}

/// Composition wiring 唯一 dataset 构造入口：concrete adapter 不进入稳定公开面。
pub fn file_system_dataset(
    root: impl AsRef<std::path::Path>,
) -> Result<Arc<dyn AtomicDatasetPort>, StorageError> {
    log::debug!(target: crate::LOG_TARGET, "file_system_dataset init enter");
    match FileSystemDatasetAdapter::new(root) {
        Ok(adapter) => {
            log::info!(target: crate::LOG_TARGET, "file_system_dataset init ok");
            Ok(Arc::new(adapter))
        }
        Err(error) => {
            log::error!(target: crate::LOG_TARGET, "file_system_dataset init failed");
            Err(error)
        }
    }
}

pub use adapters::{
    SafeOpenOptions, SafeStorageDir, SafeStorageEntry, SafeStorageFileType, SafeStorageRoot,
};
pub use domain::{
    decide_blob_recovery, decide_orphan_previous, BlobRead, CommitWarning, CorruptTransactionError,
    CorruptionReason, DatasetChangeSet, DatasetCommitReceipt, DatasetCommitVisibility, DatasetKey,
    DatasetManifest, DatasetMember, DatasetMemberChange, DatasetMemberReference, DatasetRead,
    DatasetReadOutcome, DatasetRevision, DeleteOptions, DeleteOutcome, DigestObservation,
    Durability, Generation, JournalPhase, PreviousPolicy, PromoteOutcome, QuarantineDisposition,
    QuarantineOutcome, QuarantineReason, QuarantineReceipt, ReadOutcome, RecoveryDecision,
    SafePathSegment, StorageEntry, StorageError, StorageErrorKind, StorageKey, StorageNamespace,
    TransactionDigest, TransactionScope, WriteOptions, WriteReceipt,
};
pub use ports::{AtomicBlobPort, AtomicDatasetPort};

#[cfg(test)]
#[path = "test_log.rs"]
pub(crate) mod test_log;

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
