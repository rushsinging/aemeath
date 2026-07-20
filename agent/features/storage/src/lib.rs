/// 本 crate 的日志 target。所有 log::xxx! 调用必须引用此常量。
pub(crate) const LOG_TARGET: &str = "aemeath:agent:storage";
mod adapters;
mod domain;
mod ports;

pub mod api {
    pub use crate::{
        AtomicBlobPort, AtomicDatasetPort, BlobRead, CommitWarning, DatasetCommitReceipt,
        DatasetCommitVisibility, DatasetKey, DatasetManifest, DatasetMember, DatasetRead,
        DatasetReadOutcome, DatasetRevision, DeleteOptions, DeleteOutcome, Durability, Generation,
        PreviousPolicy, PromoteOutcome, QuarantineOutcome, QuarantineReason, QuarantineReceipt,
        ReadOutcome, SafeOpenOptions, SafePathSegment, SafeStorageDir, SafeStorageEntry,
        SafeStorageFileType, SafeStorageRoot, StorageError, StorageErrorKind, StorageKey,
        StorageNamespace, TransactionScope, WriteOptions, WriteReceipt,
    };

    pub fn file_system_blob(
        root: impl AsRef<std::path::Path>,
    ) -> Result<std::sync::Arc<dyn AtomicBlobPort>, StorageError> {
        log::debug!(target: crate::LOG_TARGET, "file_system_blob init enter");
        match crate::FileSystemBlobAdapter::new(root) {
            Ok(adapter) => {
                log::info!(target: crate::LOG_TARGET, "file_system_blob init ok");
                Ok(std::sync::Arc::new(adapter))
            }
            Err(error) => {
                log::error!(target: crate::LOG_TARGET, "file_system_blob init failed");
                Err(error)
            }
        }
    }
}

pub use adapters::{
    FileSystemBlobAdapter, FileSystemDatasetAdapter, SafeOpenOptions, SafeStorageDir,
    SafeStorageEntry, SafeStorageFileType, SafeStorageRoot,
};
pub use domain::{
    decide_blob_recovery, decide_orphan_previous, BlobRead, CommitWarning, CorruptTransactionError,
    CorruptionReason, DatasetCommitReceipt, DatasetCommitVisibility, DatasetKey, DatasetManifest,
    DatasetMember, DatasetRead, DatasetReadOutcome, DatasetRevision, DeleteOptions, DeleteOutcome,
    DigestObservation, Durability, Generation, JournalPhase, PreviousPolicy, PromoteOutcome,
    QuarantineDisposition, QuarantineOutcome, QuarantineReason, QuarantineReceipt, ReadOutcome,
    RecoveryDecision, SafePathSegment, StorageError, StorageErrorKind, StorageKey,
    StorageNamespace, TransactionDigest, TransactionScope, WriteOptions, WriteReceipt,
};
pub use ports::{AtomicBlobPort, AtomicDatasetPort};

#[cfg(test)]
#[path = "test_log.rs"]
pub(crate) mod test_log;

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
