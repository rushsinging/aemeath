use super::types::{MemorySuggestion, ReflectionApplyResult, ReflectionOutput, ReflectionResult};
use share::memory::{MemoryEntry, MemoryLayer, MemorySource, MemoryStore};

pub fn apply_suggestions(
    suggestions: &[MemorySuggestion],
    store: &mut MemoryStore,
) -> ReflectionResult<usize> {
    let mut added = 0;
    for suggestion in suggestions {
        let mut entry = MemoryEntry::new(
            MemoryLayer::Project,
            suggestion.category,
            suggestion.content.clone(),
            MemorySource::Llm,
        );
        entry.tags = suggestion.tags.clone();
        store.add(entry)?;
        added += 1;
    }
    Ok(added)
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
