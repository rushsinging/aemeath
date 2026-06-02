//! memory 共享内核的公共接口（DTO / 枚举 / error / 纯函数）。
//!
//! 带文件系统 IO 的持久化（MemoryStore、memory_base_dir、project_file_name_from_path）
//! 已归位 `storage::memory`（047 spec §13），不再经此处转发。

pub use crate::memory::{
    format_add_result, format_memory_list, parse_category, parse_layer, AddResult, CompactResult,
    MemoryCategory, MemoryEntry, MemoryLayer, MemorySource, MemoryStats,
};
