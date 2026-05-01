//! Memory 系统

pub mod dedup;
pub mod entry;
pub mod error;
pub mod format;
pub mod path;
pub mod scoring;
pub mod session_reminder;
pub mod store;

pub use entry::{MemoryCategory, MemoryEntry, MemoryLayer, MemorySource};
pub use error::{MemoryError, MemoryResult};
pub use format::{format_add_result, format_memory_list, parse_category, parse_layer, short_id};
pub use path::{memory_base_dir, project_hash, project_hash_from_path};
pub use session_reminder::{SessionReminder, SessionReminders};
pub use store::{AddResult, CompactResult, MemoryStats, MemoryStore};

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store(max_entries: usize) -> (MemoryStore, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "aemeath-memory-test-{}",
            uuid::Uuid::new_v4()
        ));
        let store = MemoryStore::new(&dir, "project", max_entries, 0.8).unwrap();
        (store, dir)
    }

    fn project_entry(content: &str) -> MemoryEntry {
        MemoryEntry::new(
            MemoryLayer::Project,
            MemoryCategory::Decision,
            content,
            MemorySource::User,
        )
    }

    #[test]
    fn test_memory_store_add_and_search() {
        let (mut store, dir) = temp_store(10);
        store.add(project_entry("统一使用 AemeathError 处理错误")).unwrap();

        let results = store.search("AemeathError", 10).unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].content, "统一使用 AemeathError 处理错误");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn test_memory_store_add_empty_error() {
        let (mut store, dir) = temp_store(10);
        let result = store.add(project_entry("   "));

        assert!(matches!(result, Err(MemoryError::InvalidInput { .. })));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn test_memory_store_add_similar_merges() {
        let (mut store, dir) = temp_store(10);
        store.add(project_entry("rust error handling pattern")).unwrap();
        let result = store.add(project_entry("rust error handling pattern")).unwrap();

        assert!(matches!(result, AddResult::Merged { .. }));
        assert_eq!(store.list(None).unwrap().len(), 1);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn test_memory_store_delete_not_found() {
        let (mut store, dir) = temp_store(10);
        let result = store.delete("missing");

        assert!(matches!(result, Err(MemoryError::NotFound { .. })));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn test_memory_store_pin_excludes_eviction() {
        let (mut store, dir) = temp_store(2);
        let first = project_entry("first memory");
        let first_id = first.id.clone();
        store.add(first).unwrap();
        store.add(project_entry("second memory")).unwrap();
        store.pin(&first_id, true).unwrap();

        let candidates = store.eviction_candidates(MemoryLayer::Project, 2).unwrap();

        assert!(candidates.iter().all(|entry| entry.id != first_id));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn test_memory_store_mark_outdated_lowers_inject_rank() {
        let (mut store, dir) = temp_store(10);
        let old = project_entry("old decision");
        let old_id = old.id.clone();
        let mut active = project_entry("active decision");
        active.access_count = 1;
        store.add(old).unwrap();
        store.add(active).unwrap();
        store.mark_outdated(&old_id).unwrap();

        let top = store.top_for_inject(2).unwrap();

        assert_eq!(top[0].content, "active decision");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn test_memory_store_top_for_inject_touches_entries() {
        let (mut store, dir) = temp_store(10);
        let entry = project_entry("inject me");
        let id = entry.id.clone();
        store.add(entry).unwrap();

        let top = store.top_for_inject(1).unwrap();
        let stored = store.list(None).unwrap().into_iter().find(|entry| entry.id == id).unwrap();

        assert_eq!(top.len(), 1);
        assert_eq!(stored.access_count, 1);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn test_memory_store_archive_entries() {
        let (mut store, dir) = temp_store(10);
        let entry = project_entry("archive me");
        let id = entry.id.clone();
        store.add(entry).unwrap();

        store.archive_entries(std::slice::from_ref(&id)).unwrap();

        assert!(store.list(None).unwrap().is_empty());
        assert_eq!(store.search("archive", 10).unwrap().len(), 1);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn test_memory_store_stats() {
        let (mut store, dir) = temp_store(10);
        store.add(project_entry("project memory")).unwrap();

        let stats = store.stats(2).unwrap();

        assert_eq!(stats.project_count, 1);
        assert_eq!(stats.reminders_count, 2);
        let _ = std::fs::remove_dir_all(dir);
    }
}
