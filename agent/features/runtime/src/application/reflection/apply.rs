//! Reflection apply helpers backed by a shared `memory::MemoryPort`.
//!
//! The runtime never opens `storage::MemoryStore` directly: every reflection
//! write/mutation flows through the same `MemoryPort` the caller bound (Main Run
//! uses `BoundMainRun::memory`; forced/idle operations acquire the committed
//! memory under `wiring.with_shared`). This keeps reflection consistent with the
//! Memory BC and respects the session-switch gate.

use super::types::{
    MemorySuggestion, ReflectionApplyResult, ReflectionError, ReflectionOutput, ReflectionResult,
};
use memory::{MemoryEntry, MemoryId, MemoryPort};

fn current_timestamp_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

/// `MemorySuggestion` carries `share::memory` enums (the LLM JSON contract),
/// while the `MemoryPort` operates on `memory`-crate enums. The two enums share
/// identical variants, so this is a lossless 1:1 mapping.
fn suggestion_layer(layer: share::memory::MemoryLayer) -> memory::MemoryLayer {
    match layer {
        share::memory::MemoryLayer::Global => memory::MemoryLayer::Global,
        share::memory::MemoryLayer::Project => memory::MemoryLayer::Project,
    }
}

fn suggestion_category(category: share::memory::MemoryCategory) -> memory::MemoryCategory {
    match category {
        share::memory::MemoryCategory::Fact => memory::MemoryCategory::Fact,
        share::memory::MemoryCategory::Decision => memory::MemoryCategory::Decision,
        share::memory::MemoryCategory::Preference => memory::MemoryCategory::Preference,
        share::memory::MemoryCategory::Pattern => memory::MemoryCategory::Pattern,
        share::memory::MemoryCategory::Pitfall => memory::MemoryCategory::Pitfall,
    }
}

/// Writes each suggestion to `port`, returning how many were added or merged.
///
/// Each suggestion becomes a fresh `MemoryEntry` (`MemoryId::now_v7`, source =
/// `MemorySource::Llm`) carrying the suggestion's layer/category/tags. If the
/// layer is full the candidate eviction entries are archived once and the write
/// retried; a pinned-only full layer surfaces as a structured error.
pub async fn apply_suggestions_via_port(
    suggestions: &[MemorySuggestion],
    port: &dyn MemoryPort,
) -> ReflectionResult<usize> {
    let mut added = 0;
    for suggestion in suggestions {
        let now = current_timestamp_secs();
        let mut entry = MemoryEntry::new(
            MemoryId::now_v7(),
            now,
            suggestion_layer(suggestion.layer),
            suggestion_category(suggestion.category),
            suggestion.content.clone(),
            memory::MemorySource::Llm,
        )?;
        entry.tags = suggestion.tags.clone();
        if add_with_eviction_retry(port, entry).await? {
            added += 1;
        }
    }
    Ok(added)
}

async fn add_with_eviction_retry(
    port: &dyn MemoryPort,
    entry: MemoryEntry,
) -> ReflectionResult<bool> {
    match port.write(entry.clone()).await? {
        memory::WriteResult::Added { .. } | memory::WriteResult::Merged { .. } => Ok(true),
        memory::WriteResult::NeedsEviction { candidates } => {
            let ids = candidates
                .iter()
                .map(|candidate| candidate.id)
                .collect::<Vec<_>>();
            port.archive(&ids).await?;
            match port.write(entry).await? {
                memory::WriteResult::Added { .. } | memory::WriteResult::Merged { .. } => Ok(true),
                memory::WriteResult::NeedsEviction { candidates } => {
                    Err(ReflectionError::Apply(format!(
                        "store still requires eviction after archiving {} candidate(s); {} new candidate(s) remain",
                        ids.len(),
                        candidates.len()
                    )))
                }
                memory::WriteResult::NoOp => Ok(false),
            }
        }
        memory::WriteResult::NoOp => Ok(false),
    }
}

/// Marks each listed memory ID as outdated, returning how many were marked.
///
/// IDs are parsed to `MemoryId` (invalid/non-UUID ids are skipped with a
/// warning). A valid-but-missing ID (NotFound / `Ok(false)`) is skipped rather
/// than failing the whole apply, so a stale reflection ID never aborts the
/// batch.
pub async fn apply_outdated_via_port(
    ids: &[String],
    port: &dyn MemoryPort,
) -> ReflectionResult<usize> {
    let mut marked = 0;
    for raw in ids {
        let id = match MemoryId::new(raw) {
            Ok(id) => id,
            Err(error) => {
                log::warn!(
                    target: crate::LOG_TARGET,
                    "Reflection outdated ID `{raw}` is not a valid MemoryId, skipping: {error}"
                );
                continue;
            }
        };
        match port.mark_outdated(&id).await {
            Ok(true) => marked += 1,
            Ok(false) => {
                log::warn!(
                    target: crate::LOG_TARGET,
                    "Reflection outdated ID `{id}` not found in memory, skipping"
                );
            }
            Err(memory::MemoryError::NotFound { .. }) => {
                log::warn!(
                    target: crate::LOG_TARGET,
                    "Reflection outdated ID `{id}` not found in memory, skipping"
                );
            }
            Err(error) => return Err(error.into()),
        }
    }
    Ok(marked)
}

pub async fn apply_output_via_port(
    output: &ReflectionOutput,
    port: &dyn MemoryPort,
) -> ReflectionResult<ReflectionApplyResult> {
    Ok(ReflectionApplyResult {
        suggestions_added: apply_suggestions_via_port(&output.suggested_memories, port).await?,
        outdated_marked: apply_outdated_via_port(&output.outdated_memories, port).await?,
    })
}
