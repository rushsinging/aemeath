use crate::*;
use async_trait::async_trait;
use std::sync::Arc;
use thiserror::Error;

/// One legacy member as observed by composition. Memory owns this classification
/// and deliberately exposes bytes rather than filesystem paths.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum LegacyMemoryMember {
    #[default]
    Missing,
    Present(Vec<u8>),
}

/// The legacy active/archive members belonging to one Memory layer.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LegacyMemoryLayer {
    pub active: LegacyMemoryMember,
    pub archive: LegacyMemoryMember,
}

impl LegacyMemoryLayer {
    pub fn is_present(&self) -> bool {
        matches!(self.active, LegacyMemoryMember::Present(_))
            || matches!(self.archive, LegacyMemoryMember::Present(_))
    }
}

/// Failures produced while composition obtains legacy bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum LegacyMemorySourceError {
    #[error("permission denied while reading legacy memory")]
    PermissionDenied,
    #[error("I/O failure while reading legacy memory")]
    Io,
}

/// Memory-owned legacy discovery port. Implementations may read old files, but
/// paths and Storage implementation details never cross this boundary.
#[async_trait]
pub trait LegacyMemorySource: Send + Sync {
    async fn probe(&self, layer: MemoryLayer)
        -> Result<LegacyMemoryLayer, LegacyMemorySourceError>;
}

/// Cloneable factory that creates a project-specific [`LegacyMemorySource`]
/// pre-bound to the legacy file positions for the given project identity.
///
/// `LegacyMemorySource::probe` deliberately accepts only a [`MemoryLayer`] —
/// it has no project parameter. A factory call is therefore the *only* seam at
/// which a project identity ([`ProjectMemoryKey`]) is translated into the
/// concrete legacy positions that a source instance will read from. The
/// created source exposes bytes through `probe`, never filesystem paths.
///
/// The trait is object-safe so it can be used as `dyn LegacyMemorySourceFactory`,
/// and cloneable via [`LegacyMemorySourceFactory::boxed_clone`]; `Box<dyn
/// LegacyMemorySourceFactory>` implements [`Clone`].
pub trait LegacyMemorySourceFactory: Send + Sync {
    /// Creates a [`LegacyMemorySource`] pre-bound to the legacy positions for
    /// `key`. This is infallible: it merely resolves positions; all I/O (and
    /// potential errors) happen lazily inside [`LegacyMemorySource::probe`].
    fn create_for(&self, key: &ProjectMemoryKey) -> Arc<dyn LegacyMemorySource>;

    /// Object-safe clone — returns a boxed duplicate with identical wiring.
    fn boxed_clone(&self) -> Box<dyn LegacyMemorySourceFactory>;
}

/// Makes `Box<dyn LegacyMemorySourceFactory>` cloneable by delegating to
/// [`LegacyMemorySourceFactory::boxed_clone`].
impl Clone for Box<dyn LegacyMemorySourceFactory> {
    fn clone(&self) -> Self {
        self.boxed_clone()
    }
}

/// Fail-closed errors from opening and, when necessary, migrating Memory.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum MemoryOpenerError {
    #[error("permission denied while opening memory")]
    PermissionDenied,
    #[error("memory transaction is corrupt")]
    CorruptTransaction,
    #[error("memory dataset is corrupt")]
    CorruptDataset,
    #[error("unsupported memory schema version: {version}")]
    UnsupportedSchema { version: u32 },
    #[error("new and legacy memory keys both contain data")]
    LegacyKeyConflict,
    #[error("legacy memory migration was not committed")]
    LegacyMigrationFailed,
    #[error("I/O failure while opening memory")]
    Io,
}

/// One validated Memory-owned layer dataset and the Storage revision at which
/// it was read. Each layer owns an independent generation, so the revision is
/// scoped to a single layer and is intentionally opaque to Memory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommittedMemoryDataset<R> {
    pub dataset: MemoryDataset,
    pub revision: R,
}

/// Visibility of a successful commit. Both variants are committed outcomes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryCommitVisibility {
    Visible,
    RecoveryPending,
}

/// Typed proof that a candidate became the committed dataset generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryCommitReceipt<R> {
    revision: R,
    visibility: MemoryCommitVisibility,
}

impl<R> MemoryCommitReceipt<R> {
    pub fn new(revision: R, visibility: MemoryCommitVisibility) -> Self {
        Self {
            revision,
            visibility,
        }
    }

    pub fn revision(&self) -> &R {
        &self.revision
    }

    pub fn visibility(&self) -> MemoryCommitVisibility {
        self.visibility
    }

    pub fn into_revision(self) -> R {
        self.revision
    }
}

/// Memory-owned persistence seam. Implementations translate this contract to
/// Storage primitives; the Memory core never depends on Storage types. Each
/// `MemoryLayer` is an independent generation with its own revision, so global
/// and project memory are loaded and committed separately.
#[async_trait]
pub trait MemoryDatasetStore: Send + Sync {
    type Revision: Clone + Send + Sync + 'static;

    async fn load_committed(
        &self,
        layer: MemoryLayer,
    ) -> Result<CommittedMemoryDataset<Self::Revision>, MemoryError>;

    /// Atomically commits a single layer using revision CAS. An `Err` means
    /// NotCommitted. Both receipt visibility variants mean Committed.
    async fn commit(
        &self,
        layer: MemoryLayer,
        expected: &Self::Revision,
        dataset: &MemoryDataset,
    ) -> Result<MemoryCommitReceipt<Self::Revision>, MemoryError>;
}

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

#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct ReflectionApplyResult {
    /// Number of requested operations (suggestions plus outdated-memory marks).
    pub attempted: usize,
    /// Number of operations durably completed. This can be smaller than
    /// `attempted` when a cross-layer apply returns `MemoryError::PartialApply`.
    pub completed: usize,
    pub suggestions_added: usize,
    pub outdated_marked: usize,
}

pub trait ReflectionPromptPort: Send + Sync {
    fn build_prompt(&self, project_memory: &str, recent_summary: &str, lang: &str) -> String;
    fn parse_output(&self, raw: &str) -> ReflectionResult<ReflectionOutput>;
    fn format_output(&self, output: &ReflectionOutput, lang: &str) -> String;
    fn format_memory_summary(&self, entries: &[MemoryEntry]) -> String;
    fn recent_messages_summary(&self, messages: &[ReflectionMessage], max_chars: usize) -> String;
}

#[async_trait]
pub trait ReflectionHistoryQuery: Send + Sync {
    /// Returns at most `limit` records, newest append first. A zero limit
    /// returns an empty result without weakening dataset validation.
    async fn list(&self, limit: usize) -> Result<Vec<ReflectionRecord>, MemoryError>;
}

/// Memory-owned write boundary for completed Reflection facts. Implementations
/// persist `ReflectionRecord` only; provider prompts and raw responses cannot
/// cross this typed boundary.
#[async_trait]
pub trait ReflectionHistoryStore: ReflectionHistoryQuery {
    async fn append(&self, record: &ReflectionRecord) -> Result<(), MemoryError>;
    /// Inserts a new record or replaces the record with the same stable id.
    async fn upsert(&self, record: &ReflectionRecord) -> Result<(), MemoryError>;
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

/// Object-safe, cloneable project-aware Memory opener seam.
///
/// Composition supplies a Project-owned identity ([`ProjectMemoryKey`]) and a
/// candidate [`share::config::MemoryConfig`]; the opener eagerly opens both
/// layers and returns `Arc<dyn MemoryPort>`.
///
/// Memory never imports the Config *service* or reads the *current* config —
/// the candidate `MemoryConfig` is passed by value by the caller, decoupling
/// Memory from Config's lifecycle. Memory only depends on the plain config
/// *types* in `share::config`.
///
/// The trait is object-safe so it can be used as `dyn MemoryOpener`, and
/// cloneable via [`MemoryOpener::boxed_clone`]; `Box<dyn MemoryOpener>`
/// implements [`Clone`].
#[async_trait]
pub trait MemoryOpener: Send + Sync {
    /// Eagerly opens the global and project layers for the given project
    /// identity, using `config` to derive the [`MemoryPolicy`], and returns
    /// a fully initialized Memory port.
    async fn open_memory(
        &self,
        key: &ProjectMemoryKey,
        config: &share::config::MemoryConfig,
    ) -> Result<Arc<dyn MemoryPort>, MemoryOpenerError>;

    /// Object-safe clone — returns a boxed duplicate with identical wiring.
    fn boxed_clone(&self) -> Box<dyn MemoryOpener>;
}

/// Makes `Box<dyn MemoryOpener>` cloneable by delegating to
/// [`MemoryOpener::boxed_clone`].
impl Clone for Box<dyn MemoryOpener> {
    fn clone(&self) -> Self {
        self.boxed_clone()
    }
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
