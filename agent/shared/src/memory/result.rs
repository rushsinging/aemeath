//! Memory 操作结果 DTO（纯数据，无 IO）。
//!
//! 这些类型描述 legacy Memory 操作结果，本身不含文件系统 IO；持久化与
//! 查询行为已由 Memory BC 的 `MemoryPort` / service 接管。

use super::MemoryEntry;

#[derive(Debug, Clone, PartialEq)]
pub enum AddResult {
    Added { id: String },
    Merged { existing_id: String },
    NeedsEviction { candidates: Vec<MemoryEntry> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompactResult {
    pub archived: usize,
    pub remaining: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryStats {
    pub global_count: usize,
    pub global_archive_count: usize,
    pub project_count: usize,
    pub project_archive_count: usize,
    pub reminders_count: usize,
}

#[cfg(test)]
#[path = "result_tests.rs"]
mod tests;
