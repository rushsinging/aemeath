//! Memory crate 的唯一发布面。见 crate 根文档的 DDD 五类说明。
//!
//! 子发布语言模块（模块即边界，词汇整体归子命名空间）：
//! - [`reflection`]：Reflection 编排词汇（workflow/记录/用量/错误分类）
//! - [`legacy`]：legacy 记忆发现端口及其签名关联类型
//! - [`search`]：显式搜索词汇

pub mod legacy {
    pub use crate::ports::{
        LegacyMemoryLayer, LegacyMemoryMember, LegacyMemorySource, LegacyMemorySourceError,
        LegacyMemorySourceFactory,
    };
}

pub mod reflection {
    pub use crate::application::{
        ReflectionExecutionIdentity, ReflectionExecutionResult, ReflectionWorkflow,
        ReflectionWorkflowError,
    };
    pub use crate::domain::{
        ReflectionApplyStatus, ReflectionErrorCategory, ReflectionOutput, ReflectionPrompt,
        ReflectionRecord, ReflectionReferenceTable, ReflectionSafeSummary, ReflectionStatus,
        ReflectionTokenUsage, ReflectionTrigger,
    };
    pub use crate::ports::ReflectionApplyResult;
}

pub mod search {
    pub use crate::application::recall::{recall_relevant, RecalledMemory};
    pub use crate::ports::{
        MemoryRetrievalMode, MemorySearchHit, MemorySearchQuery, MemorySearchResult,
    };
}

pub use crate::adapters::{InMemoryMemory, MemoryPolicy};
pub use crate::domain::{
    MemoryCategory, MemoryEntry, MemoryError, MemoryId, MemoryKind, MemoryLayer, MemoryOpenError,
    MemorySource, MemoryStorageErrorKind, MemorySuggestion, ProjectMemoryKey,
};
pub use crate::noop::NoOpMemory;
pub use crate::ports::{
    CompactResult, EvictionCandidate, MemoryLocation, MemoryOpener, MemoryOpenerError, MemoryPort,
    MemoryQuery, MemoryStats, ReflectionHistoryQuery, ReflectionHistoryStore, RestoreResult,
    WriteResult,
};
