use async_trait::async_trait;

use crate::domain::{
    DatasetCommitReceipt, DatasetKey, DatasetManifest, DatasetMember, DatasetReadOutcome,
    DatasetRevision, Generation, QuarantineOutcome, QuarantineReason, SafePathSegment,
    StorageError, TransactionScope, WriteOptions,
};

/// Storage-owned OHS for crash-consistent, complete-generation datasets.
#[async_trait]
pub trait AtomicDatasetPort: Send + Sync {
    /// Recovers any pending transaction, then discovers the current generation.
    async fn read_manifest(&self, dataset: &DatasetKey) -> Result<DatasetManifest, StorageError>;

    /// Reads exactly the requested members from the current generation without
    /// falling back to the previous generation.
    async fn read_consistent(
        &self,
        dataset: &DatasetKey,
        members: &[SafePathSegment],
    ) -> Result<DatasetReadOutcome, StorageError>;

    /// Explicitly reads requested members from the retained previous generation.
    async fn read_previous(
        &self,
        dataset: &DatasetKey,
        members: &[SafePathSegment],
    ) -> Result<DatasetReadOutcome, StorageError>;

    /// Atomically replaces the complete generation when `expected` still
    /// matches. `Ok` always means committed; `Err` means not committed, except
    /// for typed corruption where committed evidence cannot be materialized.
    async fn commit_atomic(
        &self,
        dataset: &DatasetKey,
        expected: &DatasetRevision,
        members: &[DatasetMember],
        options: WriteOptions,
    ) -> Result<DatasetCommitReceipt, StorageError>;

    /// Promotes the complete retained previous generation to primary.
    async fn promote_previous(
        &self,
        dataset: &DatasetKey,
    ) -> Result<DatasetCommitReceipt, StorageError>;

    /// Quarantines only the explicitly requested dataset generation.
    async fn quarantine(
        &self,
        dataset: &DatasetKey,
        generation: Generation,
        scope: TransactionScope,
        reason: QuarantineReason,
    ) -> Result<QuarantineOutcome, StorageError>;
}
