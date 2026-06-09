use super::types::{
    MemorySuggestion, ReflectionApplyResult, ReflectionError, ReflectionOutput, ReflectionResult,
};
use share::memory::{AddResult, MemoryEntry, MemorySource};
use storage::api::MemoryStore;

pub fn apply_suggestions(
    suggestions: &[MemorySuggestion],
    store: &mut MemoryStore,
) -> ReflectionResult<usize> {
    let mut added = 0;
    for suggestion in suggestions {
        let now = current_timestamp_secs();
        let mut entry = MemoryEntry::new(
            uuid::Uuid::now_v7().to_string(),
            now,
            suggestion.layer,
            suggestion.category,
            suggestion.content.clone(),
            MemorySource::Llm,
        );
        entry.tags = suggestion.tags.clone();
        if add_with_eviction_retry(store, entry)? {
            added += 1;
        }
    }
    Ok(added)
}

fn add_with_eviction_retry(store: &mut MemoryStore, entry: MemoryEntry) -> ReflectionResult<bool> {
    match store.add(entry.clone())? {
        AddResult::Added { .. } | AddResult::Merged { .. } => Ok(true),
        AddResult::NeedsEviction { candidates } => {
            let ids = candidates
                .into_iter()
                .map(|entry| entry.id)
                .collect::<Vec<_>>();
            store.evict(&ids)?;
            match store.add(entry)? {
                AddResult::Added { .. } | AddResult::Merged { .. } => Ok(true),
                AddResult::NeedsEviction { candidates } => Err(ReflectionError::Apply(format!(
                    "store still requires eviction after evicting {} candidate(s); {} new candidate(s) remain",
                    ids.len(),
                    candidates.len()
                ))),
            }
        }
    }
}

fn current_timestamp_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

pub fn apply_outdated(ids: &[String], store: &mut MemoryStore) -> ReflectionResult<usize> {
    let mut marked = 0;
    for id in ids {
        store.mark_outdated(id)?;
        marked += 1;
    }
    Ok(marked)
}

pub fn apply_output(
    output: &ReflectionOutput,
    store: &mut MemoryStore,
) -> ReflectionResult<ReflectionApplyResult> {
    Ok(ReflectionApplyResult {
        suggestions_added: apply_suggestions(&output.suggested_memories, store)?,
        outdated_marked: apply_outdated(&output.outdated_memories, store)?,
    })
}
