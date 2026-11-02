//! Memory 测试实现面（#2146 实现权治理：MemoryPort 实现权归本 crate，
//! 跨 crate 测试经此获得替身；生产不编译）。

use std::sync::Mutex;

use crate::api::reflection::{ReflectionApplyResult, ReflectionOutput};
use crate::api::search::{MemorySearchQuery, MemorySearchResult};
use crate::api::{
    CompactResult, MemoryEntry, MemoryError, MemoryId, MemoryLayer, MemoryPort, MemoryQuery,
    MemoryStats, MemoryWritePermit, RestoreResult, WriteResult,
};
use async_trait::async_trait;

/// 注入通道测试替身：`retrieve_for_inject` 返回固定结果并记录 query；
/// 其余操作 panic（防测试路径误用写通道）。
pub struct InjectionOnlyMemory {
    result: MemorySearchResult,
    queries: Mutex<Vec<MemoryQuery>>,
}

impl InjectionOnlyMemory {
    pub fn new(result: MemorySearchResult) -> Self {
        Self {
            result,
            queries: Mutex::new(Vec::new()),
        }
    }
    pub fn queries(&self) -> Vec<MemoryQuery> {
        self.queries.lock().unwrap().clone()
    }
}

#[async_trait]
impl MemoryPort for InjectionOnlyMemory {
    async fn acquire_write_permit(&self) -> Result<MemoryWritePermit, MemoryError> {
        panic!("context injection must not mutate memory")
    }
    async fn retrieve_for_inject(&self, query: &MemoryQuery) -> MemorySearchResult {
        self.queries.lock().unwrap().push(query.clone());
        self.result.clone()
    }
    async fn search(&self, _query: &MemorySearchQuery) -> MemorySearchResult {
        panic!("search must not be used for context injection")
    }
    async fn write(&self, _entry: MemoryEntry) -> Result<WriteResult, MemoryError> {
        panic!("context injection must not mutate memory")
    }
    async fn update(&self, _id: &MemoryId, _content: &str) -> Result<bool, MemoryError> {
        panic!("context injection must not mutate memory")
    }
    async fn delete(&self, _id: &MemoryId) -> Result<bool, MemoryError> {
        panic!("context injection must not mutate memory")
    }
    async fn pin(&self, _id: &MemoryId, _pinned: bool) -> Result<bool, MemoryError> {
        panic!("context injection must not mutate memory")
    }
    async fn mark_outdated(&self, _id: &MemoryId) -> Result<bool, MemoryError> {
        panic!("context injection must not mutate memory")
    }
    async fn apply_reflection(
        &self,
        _output: &ReflectionOutput,
    ) -> Result<ReflectionApplyResult, MemoryError> {
        panic!("context injection must not mutate memory")
    }
    async fn archive(&self, _ids: &[MemoryId]) -> Result<bool, MemoryError> {
        panic!("context injection must not mutate memory")
    }
    async fn restore(&self, _id: &MemoryId) -> Result<RestoreResult, MemoryError> {
        panic!("context injection must not mutate memory")
    }
    async fn compact(&self) -> Result<CompactResult, MemoryError> {
        panic!("context injection must not mutate memory")
    }
    async fn list(&self, _layer: Option<MemoryLayer>) -> Vec<MemoryEntry> {
        panic!("context injection must use retrieve_for_inject")
    }
    async fn stats(&self) -> MemoryStats {
        panic!("context injection must use retrieve_for_inject")
    }
}

// #2146 密封解锁：实现权归属声明（crate 内）。
impl crate::sealed::Sealed for InjectionOnlyMemory {}
