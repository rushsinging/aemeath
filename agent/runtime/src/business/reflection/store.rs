use share::memory::{MemoryEntry, MemoryLayer};
use storage::api::MemoryStore;

pub fn memory_summary(entries: &[MemoryEntry]) -> String {
    entries
        .iter()
        .map(|entry| {
            format!(
                "- [{:?}][{}] {}",
                entry.category,
                entry.tags.join(","),
                entry.content
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn project_memory_summary(store: &MemoryStore) -> String {
    let entries = store
        .list(Some(MemoryLayer::Project))
        .unwrap_or_else(|_| Vec::new());
    memory_summary(&entries)
}
