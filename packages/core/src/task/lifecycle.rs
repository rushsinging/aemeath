//! Feature #25 — Scene detection for batch lifecycle management.
//!
//! All functions are pure (no I/O) to facilitate unit testing.

use super::types::{Batch, BatchStatus, Task, TaskStatus};

// ── Result types ──────────────────────────────────────────────────────

/// Information about an interrupted batch that still has incomplete tasks.
pub struct InterruptedBatchInfo {
    pub batch_id: u64,
    pub incomplete_count: usize,
    pub incomplete_ids: Vec<String>,
}

/// Information about a stale batch that has been silent for too many turns.
pub struct StaleBatchInfo {
    pub batch_id: u64,
    pub silence_turns: u64,
    pub incomplete_ids: Vec<String>,
}

// ── Detection functions ───────────────────────────────────────────────

/// Scene 1: Detect whether the *previous* batch has all its non-deleted
/// tasks completed. Returns `Some(batch_id)` when the batch should be
/// auto-archived + a toast shown; `None` otherwise.
///
/// Edge cases:
/// - `prev_batch == None` → nothing to archive → `None`
/// - No tasks belong to that batch → `None` (nothing meaningful)
/// - All tasks are `Deleted` → `None` (effectively empty batch)
pub fn detect_batch_all_completed(
    prev_batch: Option<u64>,
    tasks: &[Task],
    _batches: &[Batch],
) -> Option<u64> {
    let batch_id = prev_batch?;

    let relevant: Vec<&Task> = tasks
        .iter()
        .filter(|t| t.batch == batch_id && t.status != TaskStatus::Deleted)
        .collect();

    // No non-deleted tasks → nothing meaningful to archive
    if relevant.is_empty() {
        return None;
    }

    let all_completed = relevant.iter().all(|t| t.status == TaskStatus::Completed);

    if all_completed {
        Some(batch_id)
    } else {
        None
    }
}

/// Scene 2: Detect whether there is an older active batch with incomplete
/// tasks that got interrupted by a new topic.
///
/// Only returns `Some(...)` when `is_new_topic == true` **and** at least one
/// active batch (other than `current_batch`) has incomplete tasks.
pub fn detect_interrupted_batch(
    current_batch: u64,
    tasks: &[Task],
    batches: &[Batch],
    is_new_topic: bool,
) -> Option<InterruptedBatchInfo> {
    if !is_new_topic {
        return None;
    }

    // Find the first active batch (other than current) with incomplete tasks.
    for batch in batches {
        if batch.id == current_batch || batch.status != BatchStatus::Active {
            continue;
        }

        let incomplete: Vec<&Task> = tasks
            .iter()
            .filter(|t| {
                t.batch == batch.id
                    && t.status != TaskStatus::Completed
                    && t.status != TaskStatus::Deleted
            })
            .collect();

        if !incomplete.is_empty() {
            return Some(InterruptedBatchInfo {
                batch_id: batch.id,
                incomplete_count: incomplete.len(),
                incomplete_ids: incomplete.iter().map(|t| t.id.clone()).collect(),
            });
        }
    }

    None
}

/// Scene 3: Detect batches that have been silent for at least `threshold`
/// turns and still contain incomplete tasks.
///
/// Returns one entry per qualifying active batch.
pub fn detect_stale_batches(
    tasks: &[Task],
    batches: &[Batch],
    threshold: usize,
) -> Vec<StaleBatchInfo> {
    let mut result = Vec::new();

    for batch in batches {
        if batch.status != BatchStatus::Active {
            continue;
        }
        if (batch.silence_turns as usize) < threshold {
            continue;
        }

        let incomplete: Vec<&Task> = tasks
            .iter()
            .filter(|t| {
                t.batch == batch.id
                    && t.status != TaskStatus::Completed
                    && t.status != TaskStatus::Deleted
            })
            .collect();

        if !incomplete.is_empty() {
            result.push(StaleBatchInfo {
                batch_id: batch.id,
                silence_turns: batch.silence_turns,
                incomplete_ids: incomplete.iter().map(|t| t.id.clone()).collect(),
            });
        }
    }

    result
}

// ── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "lifecycle_tests.rs"]
mod tests;
