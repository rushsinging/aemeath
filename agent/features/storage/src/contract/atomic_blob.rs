use async_trait::async_trait;

use super::{StorageError, StorageKey};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct WriteOptions {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlobRead {
    bytes: Vec<u8>,
}

impl BlobRead {
    pub fn new(bytes: Vec<u8>) -> Self {
        Self { bytes }
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadOutcome {
    NotFound,
    Found(BlobRead),
}

/// 成功越过原子 replace 线性化点的回执。
///
/// 该回执不声明 durability、是否覆盖旧值或 Previous 状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WriteReceipt;

impl WriteReceipt {
    pub const fn committed() -> Self {
        Self
    }
}

/// opaque bytes 的原子 Blob 机制端口。
///
/// 丢弃或取消 `write_atomic` future 不会撤销已经启动的文件事务；调用方可能收不到
/// `WriteReceipt`，但 replace 仍可能完成。
#[async_trait]
pub trait AtomicBlobPort: Send + Sync {
    async fn read(&self, key: &StorageKey) -> Result<ReadOutcome, StorageError>;

    async fn write_atomic(
        &self,
        key: &StorageKey,
        bytes: &[u8],
        options: &WriteOptions,
    ) -> Result<WriteReceipt, StorageError>;
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use async_trait::async_trait;

    use super::*;
    use crate::contract::{SafePathSegment, StorageNamespace};

    struct FakePort;

    #[async_trait]
    impl AtomicBlobPort for FakePort {
        async fn read(&self, _key: &StorageKey) -> Result<ReadOutcome, StorageError> {
            Ok(ReadOutcome::Found(BlobRead::new(Vec::new())))
        }

        async fn write_atomic(
            &self,
            _key: &StorageKey,
            _bytes: &[u8],
            _options: &WriteOptions,
        ) -> Result<WriteReceipt, StorageError> {
            Ok(WriteReceipt::committed())
        }
    }

    fn key() -> StorageKey {
        StorageKey::new(
            StorageNamespace::Sessions,
            [SafePathSegment::new("session.json").unwrap()],
        )
        .unwrap()
    }

    #[tokio::test]
    async fn test_atomic_blob_port_object_safe() {
        let port: Arc<dyn AtomicBlobPort> = Arc::new(FakePort);
        assert_eq!(
            port.read(&key()).await.unwrap(),
            ReadOutcome::Found(BlobRead::new(Vec::new()))
        );
    }

    #[test]
    fn test_blob_read_distinguishes_empty_value_from_not_found() {
        let found = ReadOutcome::Found(BlobRead::new(Vec::new()));
        assert_ne!(found, ReadOutcome::NotFound);
    }

    #[test]
    fn test_blob_read_preserves_opaque_bytes() {
        let value = BlobRead::new(vec![0, 0xff, 42]);
        assert_eq!(value.bytes(), &[0, 0xff, 42]);
        assert_eq!(value.into_bytes(), vec![0, 0xff, 42]);
    }
}
