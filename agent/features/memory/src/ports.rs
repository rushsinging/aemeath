use crate::*;
use async_trait::async_trait;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryRetrievalMode {
    InjectionPriority,
    ExplicitSearch,
    Disabled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryLocation {
    Active,
    Archive,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MemorySearchHit {
    pub entry: MemoryEntry,
    pub location: MemoryLocation,
    pub outdated: bool,
    pub ttl_expired: bool,
    pub relevance: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MemorySearchResult {
    pub mode: MemoryRetrievalMode,
    pub hits: Vec<MemorySearchHit>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryQuery {
    pub limit: usize,
    pub layer: Option<MemoryLayer>,
    pub category: Option<MemoryCategory>,
    pub now: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemorySearchQuery {
    pub text: String,
    pub limit: usize,
    pub layer: Option<MemoryLayer>,
    pub category: Option<MemoryCategory>,
    pub include_archive: bool,
    pub now: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteResult {
    Added { id: MemoryId },
    Merged { existing_id: MemoryId },
    NeedsEviction { candidates: Vec<MemoryEntry> },
    NoOp,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompactResult {
    pub archived: usize,
    pub remaining: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MemoryStats {
    pub global_count: usize,
    pub global_archive_count: usize,
    pub project_count: usize,
    pub project_archive_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReflectionOutput;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ReflectionApplyResult {
    pub suggestions_added: usize,
    pub outdated_marked: usize,
}

#[async_trait]
pub trait MemoryPort: Send + Sync {
    fn retrieve_for_inject(&self, query: &MemoryQuery) -> MemorySearchResult;
    fn search(&self, query: &MemorySearchQuery) -> MemorySearchResult;
    async fn write(&self, entry: MemoryEntry) -> Result<WriteResult, MemoryError>;
    async fn update(&self, id: &MemoryId, content: &str) -> Result<bool, MemoryError>;
    async fn delete(&self, id: &MemoryId) -> Result<bool, MemoryError>;
    async fn pin(&self, id: &MemoryId, pinned: bool) -> Result<bool, MemoryError>;
    async fn mark_outdated(&self, id: &MemoryId) -> Result<bool, MemoryError>;
    async fn apply_reflection(
        &self,
        output: &ReflectionOutput,
    ) -> Result<ReflectionApplyResult, MemoryError>;
    async fn archive(&self, ids: &[MemoryId]) -> Result<(), MemoryError>;
    async fn compact(&self) -> Result<CompactResult, MemoryError>;
    fn list(&self, layer: Option<MemoryLayer>) -> Vec<MemoryEntry>;
    fn stats(&self) -> MemoryStats;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelope_preserves_mode_metadata_and_relevance() {
        let entry = MemoryEntry::new(
            MemoryId::now_v7(),
            10,
            MemoryLayer::Project,
            MemoryCategory::Fact,
            "legacy fact",
            MemorySource::User,
        )
        .unwrap();
        let result = MemorySearchResult {
            mode: MemoryRetrievalMode::ExplicitSearch,
            hits: vec![MemorySearchHit {
                entry,
                location: MemoryLocation::Archive,
                outdated: true,
                ttl_expired: true,
                relevance: Some(0.75),
            }],
        };
        assert_eq!(result.mode, MemoryRetrievalMode::ExplicitSearch);
        assert_eq!(result.hits[0].location, MemoryLocation::Archive);
        assert_eq!(result.hits[0].relevance, Some(0.75));
    }
}
