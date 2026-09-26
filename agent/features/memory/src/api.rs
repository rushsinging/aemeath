//! Memory crate 的唯一发布面。见 crate 根文档的 DDD 五类说明。

pub use crate::adapters::{InMemoryMemory, MemoryPolicy};
pub use crate::application::{
    ReflectionExecutionIdentity, ReflectionExecutionResult, ReflectionWorkflow,
    ReflectionWorkflowError,
};
pub use crate::domain::{
    MemoryCategory, MemoryEntry, MemoryError, MemoryId, MemoryLayer, MemoryOpenError, MemorySource,
    MemoryStorageErrorKind, MemorySuggestion, ProjectMemoryKey, ReflectionApplyStatus,
    ReflectionErrorCategory, ReflectionOutput, ReflectionRecord, ReflectionSafeSummary,
    ReflectionStatus, ReflectionTokenUsage, ReflectionTrigger,
};
pub use crate::noop::NoOpMemory;
pub use crate::ports::{
    CompactResult, EvictionCandidate, LegacyMemoryLayer, LegacyMemoryMember, LegacyMemorySource,
    LegacyMemorySourceError, LegacyMemorySourceFactory, MemoryLocation, MemoryOpener,
    MemoryOpenerError, MemoryPort, MemoryQuery, MemoryRetrievalMode, MemorySearchHit,
    MemorySearchQuery, MemorySearchResult, MemoryStats, ReflectionApplyResult,
    ReflectionHistoryQuery, ReflectionHistoryStore, RestoreResult, WriteResult,
};
