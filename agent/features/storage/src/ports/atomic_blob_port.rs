use async_trait::async_trait;

use crate::{
    DeleteOptions, DeleteOutcome, Generation, PromoteOutcome, QuarantineOutcome, QuarantineReason,
    ReadOutcome, StorageError, StorageKey, TransactionScope, WriteOptions, WriteReceipt,
};

#[async_trait]
pub trait AtomicBlobPort: Send + Sync {
    async fn read(
        &self,
        key: &StorageKey,
        generation: Generation,
    ) -> Result<ReadOutcome, StorageError>;

    async fn write_atomic(
        &self,
        key: &StorageKey,
        bytes: &[u8],
        options: WriteOptions,
    ) -> Result<WriteReceipt, StorageError>;

    async fn promote_previous(&self, key: &StorageKey) -> Result<PromoteOutcome, StorageError>;

    async fn quarantine(
        &self,
        key: &StorageKey,
        generation: Generation,
        scope: TransactionScope,
        reason: QuarantineReason,
    ) -> Result<QuarantineOutcome, StorageError>;

    async fn delete_all_generations(
        &self,
        key: &StorageKey,
        options: DeleteOptions,
    ) -> Result<DeleteOutcome, StorageError>;

    async fn list_primary(
        &self,
        namespace: crate::StorageNamespace,
    ) -> Result<Vec<crate::StorageEntry>, StorageError>;
}
