use crate::*;
use async_trait::async_trait;

/// 明确表示当前 Run 禁用 Memory 的空对象实现。
#[derive(Debug, Default)]
pub struct NoOpMemory;

fn disabled_result() -> MemorySearchResult {
    MemorySearchResult {
        mode: MemoryRetrievalMode::Disabled,
        hits: Vec::new(),
    }
}

#[async_trait]
impl MemoryPort for NoOpMemory {
    fn retrieve_for_inject(&self, _query: &MemoryQuery) -> MemorySearchResult {
        disabled_result()
    }

    fn search(&self, _query: &MemorySearchQuery) -> MemorySearchResult {
        disabled_result()
    }

    async fn write(&self, _entry: MemoryEntry) -> Result<WriteResult, MemoryError> {
        Ok(WriteResult::NoOp)
    }

    async fn update(&self, _id: &MemoryId, _content: &str) -> Result<bool, MemoryError> {
        Ok(false)
    }

    async fn delete(&self, _id: &MemoryId) -> Result<bool, MemoryError> {
        Ok(false)
    }

    async fn pin(&self, _id: &MemoryId, _pinned: bool) -> Result<bool, MemoryError> {
        Ok(false)
    }

    async fn mark_outdated(&self, _id: &MemoryId) -> Result<bool, MemoryError> {
        Ok(false)
    }

    async fn apply_reflection(
        &self,
        _output: &ReflectionOutput,
    ) -> Result<ReflectionApplyResult, MemoryError> {
        Ok(ReflectionApplyResult::default())
    }

    async fn archive(&self, _ids: &[MemoryId]) -> Result<(), MemoryError> {
        Ok(())
    }

    async fn compact(&self) -> Result<CompactResult, MemoryError> {
        Ok(CompactResult {
            archived: 0,
            remaining: 0,
        })
    }

    fn list(&self, _layer: Option<MemoryLayer>) -> Vec<MemoryEntry> {
        Vec::new()
    }

    fn stats(&self) -> MemoryStats {
        MemoryStats::default()
    }
}
