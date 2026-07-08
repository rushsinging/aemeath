use share::memory::entry::MemoryEntry;

pub(super) fn current_timestamp_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

pub(super) fn entry_matches(entry: &MemoryEntry, query: &str) -> bool {
    if query.trim().is_empty() {
        return true;
    }
    entry.content.to_lowercase().contains(query)
        || entry
            .tags
            .iter()
            .any(|tag| tag.to_lowercase().contains(query))
        || format!("{:?}", entry.category)
            .to_lowercase()
            .contains(query)
        || format!("{:?}", entry.layer).to_lowercase().contains(query)
}
