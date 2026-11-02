use async_trait::async_trait;

use crate::{
    DeleteOptionsData, DeleteOutcomeData, GenerationData, PromoteOutcomeData,
    QuarantineOutcomeData, QuarantineReason, ReadOutcomeData, StorageKeyData, TransactionScopeData,
    WriteOptionsData, WriteReceiptData,
};

#[async_trait]
pub trait AtomicBlobPort: Send + Sync {
    async fn read(
        &self,
        key: &StorageKeyData,
        generation: GenerationData,
    ) -> Result<ReadOutcomeData, share::error::DomainError>;

    async fn write_atomic(
        &self,
        key: &StorageKeyData,
        bytes: &[u8],
        options: WriteOptionsData,
    ) -> Result<WriteReceiptData, share::error::DomainError>;

    async fn promote_previous(
        &self,
        key: &StorageKeyData,
    ) -> Result<PromoteOutcomeData, share::error::DomainError>;

    async fn quarantine(
        &self,
        key: &StorageKeyData,
        generation: GenerationData,
        scope: TransactionScopeData,
        reason: QuarantineReason,
    ) -> Result<QuarantineOutcomeData, share::error::DomainError>;

    async fn delete_all_generations(
        &self,
        key: &StorageKeyData,
        options: DeleteOptionsData,
    ) -> Result<DeleteOutcomeData, share::error::DomainError>;

    async fn list_primary(
        &self,
        namespace: crate::domain::StorageNamespaceData,
    ) -> Result<Vec<crate::domain::StorageEntryData>, share::error::DomainError>;
}
