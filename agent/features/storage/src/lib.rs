/// 本 crate 的日志 target。所有 log::xxx! 调用必须引用此常量。
pub const LOG_TARGET: &str = "aemeath:agent:storage";

mod memory_store;
mod task_store;
mod tool_result;

pub use memory_store::{
    memory_base_dir, project_file_name, project_file_name_from_path, MemoryStore,
};
pub use task_store::{
    Batch, BatchStatus, Task, TaskPriority, TaskSnapshot, TaskStatus, TaskStore, TaskStoreStats,
};
pub use tool_result::{persist_oversized_results, MAX_TOOL_RESULT_CHARS};
