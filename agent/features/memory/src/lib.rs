#![deny(clippy::print_stdout, clippy::print_stderr)]

//! Memory 支撑域。

pub(crate) const LOG_TARGET: &str = "aemeath:agent:memory";
mod adapters;
mod application;
mod codec;
mod domain;
mod noop;
mod ports;
mod service;

pub mod api {
    pub use crate::{
        AtomicDatasetReflectionHistoryStore, CompactResult, DatasetMemoryOpener, EvictionCandidate,
        FileLegacyMemorySourceFactory, InMemoryMemory, MemoryCategory, MemoryEntry, MemoryError,
        MemoryId, MemoryLayer, MemoryLocation, MemoryOpenError, MemoryOpener, MemoryOpenerError,
        MemoryPolicy, MemoryPort, MemoryQuery, MemoryRetrievalMode, MemorySearchHit,
        MemorySearchQuery, MemorySearchResult, MemorySource, MemoryStats, MemorySuggestion,
        NoOpMemory, ProjectMemoryKey, ReflectionApplyResult, ReflectionApplyStatus,
        ReflectionEngine, ReflectionError, ReflectionErrorCategory, ReflectionExecutionIdentity,
        ReflectionExecutionResult, ReflectionHistoryQuery, ReflectionHistoryStore,
        ReflectionMessage, ReflectionOutput, ReflectionRecord, ReflectionResult,
        ReflectionSafeSummary, ReflectionStatus, ReflectionTokenUsage, ReflectionTrigger,
        ReflectionWorkflow, ReflectionWorkflowError, RestoreResult, WriteResult,
    };
}

pub use adapters::{
    AtomicDatasetReflectionHistoryStore, DatasetMemoryOpener, FileLegacyMemorySourceFactory,
    InMemoryMemory, MemoryPolicy,
};
pub use application::{
    ReflectionExecutionIdentity, ReflectionExecutionResult, ReflectionWorkflow,
    ReflectionWorkflowError,
};
pub use domain::*;
pub use noop::NoOpMemory;
pub use ports::*;
