use async_trait::async_trait;

use crate::{Generation, ReadOutcome, StorageError, StorageKey, WriteOptions, WriteReceipt};

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
}
