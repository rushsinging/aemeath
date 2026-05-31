//! Memory 操作结果 DTO（纯数据，无 IO）。
//!
//! 这些类型描述 MemoryStore 操作的结果，本身不含文件系统 IO，属共享内核；
//! 具体的持久化实现（MemoryStore）位于 `storage::memory`。

use super::MemoryEntry;

#[derive(Debug, Clone, PartialEq)]
pub enum AddResult {
    Added,
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
mod tests {
    use super::*;
    use crate::memory::{MemoryCategory, MemoryLayer, MemorySource};

    #[test]
    fn test_add_result_merged_carries_id() {
        let result = AddResult::Merged {
            existing_id: "mem-1".to_string(),
        };

        assert!(matches!(result, AddResult::Merged { existing_id } if existing_id == "mem-1"));
    }

    #[test]
    fn test_add_result_needs_eviction_holds_candidates() {
        let entry = MemoryEntry::new(
            "memory-1",
            100,
            MemoryLayer::Project,
            MemoryCategory::Fact,
            "candidate",
            MemorySource::User,
        );
        let result = AddResult::NeedsEviction {
            candidates: vec![entry],
        };

        match result {
            AddResult::NeedsEviction { candidates } => assert_eq!(candidates.len(), 1),
            _ => panic!("应为 NeedsEviction"),
        }
    }

    #[test]
    fn test_memory_stats_equality() {
        let stats = MemoryStats {
            global_count: 1,
            global_archive_count: 0,
            project_count: 2,
            project_archive_count: 1,
            reminders_count: 3,
        };

        assert_eq!(stats.clone(), stats);
        assert_eq!(stats.project_count, 2);
    }
}
