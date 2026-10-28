//! Storage 测试实现面（#2146 实现权治理：AtomicBlobPort 实现权归本 crate，
//! 跨 crate 测试经此获得替身；生产不编译）。

use std::collections::HashMap;
use std::sync::Mutex;

use crate::domain::{
    BlobReadData, DeleteOptionsData, DeleteOutcomeData, GenerationData, PromoteOutcomeData,
    QuarantineOutcomeData, QuarantineReason, ReadOutcomeData, StorageError, StorageKeyData,
    TransactionScopeData, WriteOptionsData, WriteReceiptData,
};
use crate::ports::AtomicBlobPort;
use async_trait::async_trait;

/// 内存 Blob 替身：HashMap 存储 + 读世代语义（Previous → NotFound）+
/// 可注入写失败 + writes/reads 记录（契约与快照测试共用）。
pub struct InMemoryBlobPort {
    values: Mutex<HashMap<StorageKeyData, Vec<u8>>>,
    write_failure: Mutex<Option<String>>,
    writes: Mutex<Vec<(StorageKeyData, Vec<u8>, WriteOptionsData)>>,
    reads: Mutex<Vec<GenerationData>>,
    /// (generation, scope, reason) 记录。
    quarantines: Mutex<Vec<(GenerationData, TransactionScopeData, QuarantineReason)>>,
    delete_options: Mutex<Vec<DeleteOptionsData>>,
    promote_outcome: Mutex<PromoteOutcomeData>,
}

impl Default for InMemoryBlobPort {
    fn default() -> Self {
        Self {
            values: Mutex::new(HashMap::new()),
            write_failure: Mutex::new(None),
            writes: Mutex::new(Vec::new()),
            reads: Mutex::new(Vec::new()),
            quarantines: Mutex::new(Vec::new()),
            delete_options: Mutex::new(Vec::new()),
            promote_outcome: Mutex::new(PromoteOutcomeData::AlreadyPromoted),
        }
    }
}

impl InMemoryBlobPort {
    pub fn new() -> Self {
        Self::default()
    }
    /// 注入一次写失败（之后清除）。
    pub fn fail_next_write(&self, message: impl Into<String>) {
        *self.write_failure.lock().unwrap() = Some(message.into());
    }
    pub fn values(&self) -> Vec<Vec<u8>> {
        self.values.lock().unwrap().values().cloned().collect()
    }
    pub fn writes(&self) -> Vec<(StorageKeyData, Vec<u8>, WriteOptionsData)> {
        self.writes.lock().unwrap().clone()
    }
    pub fn reads(&self) -> Vec<GenerationData> {
        self.reads.lock().unwrap().clone()
    }
    pub fn quarantines(&self) -> Vec<(GenerationData, TransactionScopeData, QuarantineReason)> {
        self.quarantines.lock().unwrap().clone()
    }
    pub fn delete_options(&self) -> Vec<DeleteOptionsData> {
        self.delete_options.lock().unwrap().clone()
    }
    /// 配置 promote_previous 返回（默认 AlreadyPromoted）。
    pub fn set_promote_outcome(&self, outcome: PromoteOutcomeData) {
        *self.promote_outcome.lock().unwrap() = outcome;
    }
}

#[async_trait]
impl AtomicBlobPort for InMemoryBlobPort {
    async fn read(
        &self,
        key: &StorageKeyData,
        generation: GenerationData,
    ) -> Result<ReadOutcomeData, StorageError> {
        self.reads.lock().unwrap().push(generation);
        if generation == GenerationData::Previous {
            return Ok(ReadOutcomeData::NotFound);
        }
        Ok(match self.values.lock().unwrap().get(key) {
            Some(bytes) => ReadOutcomeData::Found(BlobReadData::new(generation, bytes.clone())),
            None => ReadOutcomeData::NotFound,
        })
    }
    async fn write_atomic(
        &self,
        key: &StorageKeyData,
        bytes: &[u8],
        options: WriteOptionsData,
    ) -> Result<WriteReceiptData, StorageError> {
        if let Some(message) = self.write_failure.lock().unwrap().take() {
            return Err(StorageError::new(crate::StorageErrorKind::Io, message));
        }
        self.values
            .lock()
            .unwrap()
            .insert(key.clone(), bytes.to_vec());
        self.writes
            .lock()
            .unwrap()
            .push((key.clone(), bytes.to_vec(), options));
        Ok(WriteReceiptData::committed(None))
    }
    async fn promote_previous(
        &self,
        _key: &StorageKeyData,
    ) -> Result<PromoteOutcomeData, StorageError> {
        Ok(self.promote_outcome.lock().unwrap().clone())
    }
    async fn quarantine(
        &self,
        _key: &StorageKeyData,
        _generation: GenerationData,
        _scope: TransactionScopeData,
        _reason: QuarantineReason,
    ) -> Result<QuarantineOutcomeData, StorageError> {
        self.quarantines
            .lock()
            .unwrap()
            .push((_generation, _scope, _reason));
        Ok(QuarantineOutcomeData::already_absent(
            _generation,
            _scope,
            _reason,
        ))
    }
    async fn delete_all_generations(
        &self,
        key: &StorageKeyData,
        options: DeleteOptionsData,
    ) -> Result<DeleteOutcomeData, StorageError> {
        self.delete_options.lock().unwrap().push(options);
        let removed = self.values.lock().unwrap().remove(key).is_some();
        Ok(DeleteOutcomeData::new(removed, false, false))
    }
    async fn list_primary(
        &self,
        _namespace: crate::domain::StorageNamespaceData,
    ) -> Result<Vec<crate::domain::StorageEntryData>, StorageError> {
        Ok(Vec::new())
    }
}

// #2146 密封解锁。
impl crate::sealed::Sealed for InMemoryBlobPort {}
