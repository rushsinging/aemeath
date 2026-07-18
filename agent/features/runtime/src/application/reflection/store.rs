use memory::{MemoryEntry, MemoryLayer, MemoryPort};

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

/// Builds the project-memory context for the reflection prompt by reading the
/// Project layer straight from the bound `MemoryPort` — never opening legacy
/// `storage::MemoryStore`.
pub fn project_memory_summary(port: &dyn MemoryPort) -> String {
    let entries = port.list(Some(MemoryLayer::Project));
    memory_summary(&entries)
}
