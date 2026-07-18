#![deny(clippy::print_stdout, clippy::print_stderr)]

//! Memory 支撑域。

pub(crate) const LOG_TARGET: &str = "aemeath:agent:memory";
const _: &str = LOG_TARGET;
mod adapters;
mod codec;
mod domain;
mod noop;
mod ports;
mod service;

pub mod api {
    pub use crate::{
        AtomicDatasetMemoryStore, AtomicDatasetReflectionHistoryStore, CompactResult,
        InMemoryMemory, LegacyMemoryLayer, LegacyMemoryMember, LegacyMemorySource,
        LegacyMemorySourceError, MemoryCategory, MemoryEntry, MemoryError, MemoryId, MemoryLayer,
        MemoryLocation, MemoryOpenerError, MemoryPolicy, MemoryPort, MemoryQuery,
        MemoryRetrievalMode, MemorySearchHit, MemorySearchQuery, MemorySearchResult, MemorySource,
        MemoryStats, MemorySuggestion, NoOpMemory, ProjectMemoryKey, ProjectMemoryOpener,
        ReflectionApplyResult, ReflectionApplyStatus, ReflectionEngine, ReflectionError,
        ReflectionErrorCategory, ReflectionHistoryQuery, ReflectionHistoryStore, ReflectionMessage,
        ReflectionOutput, ReflectionPromptPort, ReflectionRecord, ReflectionResult,
        ReflectionSafeSummary, ReflectionStatus, ReflectionTokenUsage, ReflectionTrigger,
        WriteResult,
    };
}

pub use adapters::{
    map_storage_error, AtomicDatasetMemoryStore, AtomicDatasetReflectionHistoryStore,
    InMemoryMemory, MemoryPolicy, ProjectMemoryOpener,
};
pub use domain::*;
pub use noop::NoOpMemory;
pub use ports::*;
pub use service::MemoryService;
