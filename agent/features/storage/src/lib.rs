/// 本 crate 的日志 target。所有 log::xxx! 调用必须引用此常量。
pub const LOG_TARGET: &str = "aemeath:agent:storage";

mod adapters;
mod domain;
#[cfg(test)]
#[path = "domain_tests.rs"]
mod domain_tests;
mod memory_store;
mod ports;
mod task_store;

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
pub use memory_store::{
    memory_base_dir, project_file_name, project_file_name_from_path, MemoryStore,
};
pub use ports::{AtomicBlobPort, AtomicDatasetPort};
pub use task_store::{
    Batch, BatchStatus, Task, TaskPriority, TaskSnapshot, TaskStatus, TaskStore, TaskStoreStats,
};
