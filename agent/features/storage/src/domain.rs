mod atomic_blob;
mod atomic_dataset;
mod blob_recovery;
mod published_language;
mod safe_path;

#[cfg(test)]
#[path = "domain/domain_tests.rs"]
mod tests;

pub use atomic_blob::{
    BlobReadData, CommitWarningData, DeleteOptionsData, DeleteOutcomeData, GenerationData,
    PromoteOutcomeData, QuarantineOutcomeData, QuarantineReason, QuarantineReceiptData,
    ReadOutcomeData, StorageEntryData, TransactionScopeData, WriteOptionsData, WriteReceiptData,
};
pub(crate) use atomic_dataset::revision_member_digest;
pub use atomic_dataset::{
    DatasetChangeSetData, DatasetCommitReceiptData, DatasetCommitVisibilityData, DatasetKeyData,
    DatasetManifestData, DatasetMemberChangeData, DatasetMemberData, DatasetMemberReferenceData,
    DatasetReadData, DatasetReadOutcomeData, DatasetRevisionData,
};
#[cfg_attr(not(test), allow(unused_imports))]
pub use blob_recovery::{
    decide_blob_recovery, decide_orphan_previous, CorruptTransactionError, CorruptionReason,
    DigestObservation, JournalPhase, QuarantineDisposition, RecoveryDecision, TransactionDigest,
};
pub(crate) use published_language::PreviousPolicy;
pub use published_language::{
    DurabilityData, StorageError, StorageErrorKind, StorageKeyData, StorageNamespaceData,
};
pub use safe_path::SafePathSegmentData;
