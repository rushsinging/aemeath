use async_trait::async_trait;

use crate::domain::{
    DatasetChangeSetData, DatasetCommitReceiptData, DatasetKeyData, DatasetManifestData,
    DatasetMemberData, DatasetReadOutcomeData, DatasetRevisionData, DeleteOptionsData,
    DeleteOutcomeData, GenerationData, QuarantineOutcomeData, QuarantineReason,
    SafePathSegmentData, StorageError, TransactionScopeData, WriteOptionsData,
};

/// Storage-owned OHS for crash-consistent, complete-generation datasets.
#[async_trait]
pub trait AtomicDatasetPort: Send + Sync {
    /// Recovers any pending transaction, then discovers the current generation.
    async fn read_manifest(
        &self,
        dataset: &DatasetKeyData,
    ) -> Result<DatasetManifestData, StorageError>;

    /// Enumerates dataset keys in the namespace. Protocol artifacts and quarantine
    /// directories are excluded; the result contains only live datasets.
    async fn list_datasets(
        &self,
        namespace: crate::domain::StorageNamespaceData,
    ) -> Result<Vec<DatasetKeyData>, StorageError>;

    /// Removes the complete dataset directory, including both retained generations
    /// and optional quarantine evidence.
    async fn delete_all_generations(
        &self,
        dataset: &DatasetKeyData,
        options: DeleteOptionsData,
    ) -> Result<DeleteOutcomeData, StorageError>;

    /// falling back to the previous generation.
    async fn read_consistent(
        &self,
        dataset: &DatasetKeyData,
        members: &[SafePathSegmentData],
    ) -> Result<DatasetReadOutcomeData, StorageError>;

    /// Explicitly reads requested members from the retained previous generation.
    async fn read_previous(
        &self,
        dataset: &DatasetKeyData,
        members: &[SafePathSegmentData],
    ) -> Result<DatasetReadOutcomeData, StorageError>;

    /// Atomically replaces the complete generation when `expected` still
    /// matches. `Ok` always means committed; `Err` means not committed, except
    /// for typed corruption where committed evidence cannot be materialized.
    async fn commit_atomic(
        &self,
        dataset: &DatasetKeyData,
        expected: &DatasetRevisionData,
        members: &[DatasetMemberData],
        options: WriteOptionsData,
    ) -> Result<DatasetCommitReceiptData, StorageError>;

    /// Atomically publishes a complete target generation while carrying bytes
    /// only for changed members and reusing verified immutable members from the
    /// expected primary generation.
    async fn commit_incremental(
        &self,
        dataset: &DatasetKeyData,
        changes: &DatasetChangeSetData,
        options: WriteOptionsData,
    ) -> Result<DatasetCommitReceiptData, StorageError>;

    /// Promotes the complete retained previous generation to primary.
    async fn promote_previous(
        &self,
        dataset: &DatasetKeyData,
    ) -> Result<DatasetCommitReceiptData, StorageError>;

    /// Quarantines only the explicitly requested dataset generation.
    async fn quarantine(
        &self,
        dataset: &DatasetKeyData,
        generation: GenerationData,
        scope: TransactionScopeData,
        reason: QuarantineReason,
    ) -> Result<QuarantineOutcomeData, StorageError>;
}
