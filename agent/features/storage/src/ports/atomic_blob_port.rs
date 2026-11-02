use async_trait::async_trait;

use crate::{
    DeleteOptionsData, DeleteOutcomeData, GenerationData, PromoteOutcomeData,
    QuarantineOutcomeData, QuarantineReason, ReadOutcomeData, StorageError, StorageKeyData,
    TransactionScopeData, WriteOptionsData, WriteReceiptData,
};

#[async_trait]
pub trait AtomicBlobPort: Send + Sync {
    async fn read(
        &self,
        key: &StorageKeyData,
        generation: GenerationData,
    ) -> Result<ReadOutcomeData, StorageError>;

    async fn write_atomic(
        &self,
        key: &StorageKeyData,
        bytes: &[u8],
        options: WriteOptionsData,
    ) -> Result<WriteReceiptData, StorageError>;

    async fn promote_previous(
        &self,
        key: &StorageKeyData,
    ) -> Result<PromoteOutcomeData, StorageError>;

    async fn quarantine(
        &self,
        key: &StorageKeyData,
        generation: GenerationData,
        scope: TransactionScopeData,
        reason: QuarantineReason,
    ) -> Result<QuarantineOutcomeData, StorageError>;

    async fn delete_all_generations(
        &self,
        key: &StorageKeyData,
        options: DeleteOptionsData,
    ) -> Result<DeleteOutcomeData, StorageError>;

    async fn list_primary(
        &self,
        namespace: crate::domain::StorageNamespaceData,
    ) -> Result<Vec<crate::domain::StorageEntryData>, StorageError>;
}
