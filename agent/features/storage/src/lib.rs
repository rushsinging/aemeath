/// 本 crate 的日志 target。所有 log::xxx! 调用必须引用此常量。
pub const LOG_TARGET: &str = "aemeath:agent:storage";

mod adapters;
mod domain;
#[cfg(test)]
#[path = "domain_tests.rs"]
mod domain_tests;
mod memory_store;
mod ports;
mod task_store;
mod tool_result;

pub use adapters::FileSystemBlobAdapter;
pub use domain::{
    BlobRead, CommitWarning, DeleteOptions, DeleteOutcome, Durability, Generation, PreviousPolicy,
    PromoteOutcome, QuarantineOutcome, QuarantineReason, QuarantineReceipt, ReadOutcome,
    SafePathSegment, StorageError, StorageErrorKind, StorageKey, StorageNamespace,
    TransactionScope, WriteOptions, WriteReceipt,
};
pub use memory_store::{
    memory_base_dir, project_file_name, project_file_name_from_path, MemoryStore,
};
pub use ports::AtomicBlobPort;
pub use task_store::{
    Batch, BatchStatus, Task, TaskPriority, TaskSnapshot, TaskStatus, TaskStore, TaskStoreStats,
};
pub use tool_result::{persist_oversized_results, MAX_TOOL_RESULT_CHARS};
