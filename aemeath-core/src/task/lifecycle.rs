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
mod tests {
    use super::*;
    use crate::task::types::{Batch, BatchStatus, Task, TaskPriority, TaskStatus};

    // Helpers ----------------------------------------------------------------

    fn make_task(id: &str, batch: u64, status: TaskStatus) -> Task {
        Task {
            id: id.to_string(),
            subject: format!("task-{id}"),
            description: String::new(),
            status,
            active_form: None,
            owner: None,
            blocked_by: Vec::new(),
            blocks: Vec::new(),
            priority: TaskPriority::Normal,
            progress: 0,
            progress_message: None,
            created_at: 0,
            updated_at: 0,
            session_id: None,
            tags: Vec::new(),
            batch,
        }
    }

    fn make_batch(id: u64, status: BatchStatus, silence_turns: u64) -> Batch {
        Batch {
            id,
            status,
            created_at: 0,
            last_active_turn: 0,
            silence_turns,
        }
    }

    // ── detect_batch_all_completed ─────────────────────────────────────

    #[test]
    fn batch_all_completed_none_when_no_prev_batch() {
        let result = detect_batch_all_completed(None, &[], &[]);
        assert!(result.is_none());
    }

    #[test]
    fn batch_all_completed_none_when_no_tasks() {
        let result = detect_batch_all_completed(Some(1), &[], &[]);
        assert!(result.is_none());
    }

    #[test]
    fn batch_all_completed_yes_when_all_completed() {
        let tasks = vec![
            make_task("1", 1, TaskStatus::Completed),
            make_task("2", 1, TaskStatus::Completed),
        ];
        let result = detect_batch_all_completed(Some(1), &tasks, &[]);
        assert_eq!(result, Some(1));
    }

    #[test]
    fn batch_all_completed_no_when_one_in_progress() {
        let tasks = vec![
            make_task("1", 1, TaskStatus::Completed),
            make_task("2", 1, TaskStatus::InProgress),
        ];
        let result = detect_batch_all_completed(Some(1), &tasks, &[]);
        assert!(result.is_none());
    }

    #[test]
    fn batch_all_completed_ignores_deleted_tasks() {
        // Two completed + one deleted → still "all completed"
        let tasks = vec![
            make_task("1", 1, TaskStatus::Completed),
            make_task("2", 1, TaskStatus::Deleted),
        ];
        let result = detect_batch_all_completed(Some(1), &tasks, &[]);
        assert_eq!(result, Some(1));
    }

    #[test]
    fn batch_all_completed_none_when_only_deleted() {
        // All tasks deleted → treated as empty batch
        let tasks = vec![make_task("1", 1, TaskStatus::Deleted)];
        let result = detect_batch_all_completed(Some(1), &tasks, &[]);
        assert!(result.is_none());
    }

    #[test]
    fn batch_all_completed_filters_by_batch_id() {
        let tasks = vec![
            make_task("1", 1, TaskStatus::Completed),
            make_task("2", 2, TaskStatus::InProgress),
        ];
        // Only batch 1 matters, batch 2's in-progress task is irrelevant
        let result = detect_batch_all_completed(Some(1), &tasks, &[]);
        assert_eq!(result, Some(1));
    }

    #[test]
    fn batch_all_completed_mixed_statuses() {
        let tasks = vec![
            make_task("1", 1, TaskStatus::Completed),
            make_task("2", 1, TaskStatus::Pending),
            make_task("3", 1, TaskStatus::Deleted),
        ];
        let result = detect_batch_all_completed(Some(1), &tasks, &[]);
        assert!(result.is_none());
    }

    // ── detect_interrupted_batch ───────────────────────────────────────

    #[test]
    fn interrupted_none_when_not_new_topic() {
        let tasks = vec![make_task("1", 1, TaskStatus::InProgress)];
        let batches = vec![make_batch(1, BatchStatus::Active, 0)];
        let result = detect_interrupted_batch(2, &tasks, &batches, false);
        assert!(result.is_none());
    }

    #[test]
    fn interrupted_none_when_no_old_batches() {
        let result = detect_interrupted_batch(1, &[], &[], true);
        assert!(result.is_none());
    }

    #[test]
    fn interrupted_none_when_old_batch_all_completed() {
        let tasks = vec![
            make_task("1", 1, TaskStatus::Completed),
            make_task("2", 1, TaskStatus::Completed),
        ];
        let batches = vec![make_batch(1, BatchStatus::Active, 0)];
        let result = detect_interrupted_batch(2, &tasks, &batches, true);
        assert!(result.is_none());
    }

    #[test]
    fn interrupted_detects_incomplete_tasks() {
        let tasks = vec![
            make_task("1", 1, TaskStatus::Completed),
            make_task("2", 1, TaskStatus::InProgress),
            make_task("3", 1, TaskStatus::Pending),
        ];
        let batches = vec![make_batch(1, BatchStatus::Active, 0)];
        let result = detect_interrupted_batch(2, &tasks, &batches, true);

        let info = result.expect("should detect interrupted batch");
        assert_eq!(info.batch_id, 1);
        assert_eq!(info.incomplete_count, 2);
        assert_eq!(info.incomplete_ids, vec!["2", "3"]);
    }

    #[test]
    fn interrupted_ignores_archived_batches() {
        let tasks = vec![make_task("1", 1, TaskStatus::InProgress)];
        let batches = vec![make_batch(1, BatchStatus::Archived, 0)];
        let result = detect_interrupted_batch(2, &tasks, &batches, true);
        assert!(result.is_none());
    }

    #[test]
    fn interrupted_ignores_paused_batches() {
        let tasks = vec![make_task("1", 1, TaskStatus::InProgress)];
        let batches = vec![make_batch(1, BatchStatus::Paused, 0)];
        let result = detect_interrupted_batch(2, &tasks, &batches, true);
        assert!(result.is_none());
    }

    #[test]
    fn interrupted_ignores_current_batch() {
        let tasks = vec![make_task("1", 2, TaskStatus::InProgress)];
        let batches = vec![make_batch(2, BatchStatus::Active, 0)];
        let result = detect_interrupted_batch(2, &tasks, &batches, true);
        assert!(result.is_none());
    }

    #[test]
    fn interrupted_ignores_deleted_tasks() {
        let tasks = vec![make_task("1", 1, TaskStatus::Deleted)];
        let batches = vec![make_batch(1, BatchStatus::Active, 0)];
        let result = detect_interrupted_batch(2, &tasks, &batches, true);
        assert!(result.is_none());
    }

    #[test]
    fn interrupted_picks_first_qualifying_batch() {
        let tasks = vec![
            make_task("1", 1, TaskStatus::InProgress),
            make_task("2", 2, TaskStatus::InProgress),
        ];
        let batches = vec![
            make_batch(1, BatchStatus::Active, 0),
            make_batch(2, BatchStatus::Active, 0),
        ];
        let result = detect_interrupted_batch(3, &tasks, &batches, true);
        let info = result.expect("should detect");
        assert_eq!(info.batch_id, 1);
    }

    // ── detect_stale_batches ───────────────────────────────────────────

    #[test]
    fn stale_empty_when_no_batches() {
        let result = detect_stale_batches(&[], &[], 3);
        assert!(result.is_empty());
    }

    #[test]
    fn stale_empty_when_no_tasks() {
        let batches = vec![make_batch(1, BatchStatus::Active, 5)];
        let result = detect_stale_batches(&[], &batches, 3);
        assert!(result.is_empty());
    }

    #[test]
    fn stale_empty_when_below_threshold() {
        let tasks = vec![make_task("1", 1, TaskStatus::InProgress)];
        let batches = vec![make_batch(1, BatchStatus::Active, 2)];
        let result = detect_stale_batches(&tasks, &batches, 3);
        assert!(result.is_empty());
    }

    #[test]
    fn stale_detects_at_threshold() {
        let tasks = vec![
            make_task("1", 1, TaskStatus::InProgress),
            make_task("2", 1, TaskStatus::Pending),
        ];
        let batches = vec![make_batch(1, BatchStatus::Active, 3)];
        let result = detect_stale_batches(&tasks, &batches, 3);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].batch_id, 1);
        assert_eq!(result[0].silence_turns, 3);
        assert_eq!(result[0].incomplete_ids, vec!["1", "2"]);
    }

    #[test]
    fn stale_above_threshold() {
        let tasks = vec![make_task("1", 1, TaskStatus::Pending)];
        let batches = vec![make_batch(1, BatchStatus::Active, 10)];
        let result = detect_stale_batches(&tasks, &batches, 5);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].silence_turns, 10);
    }

    #[test]
    fn stale_ignores_archived_batches() {
        let tasks = vec![make_task("1", 1, TaskStatus::Pending)];
        let batches = vec![make_batch(1, BatchStatus::Archived, 10)];
        let result = detect_stale_batches(&tasks, &batches, 5);
        assert!(result.is_empty());
    }

    #[test]
    fn stale_ignores_paused_batches() {
        let tasks = vec![make_task("1", 1, TaskStatus::Pending)];
        let batches = vec![make_batch(1, BatchStatus::Paused, 10)];
        let result = detect_stale_batches(&tasks, &batches, 5);
        assert!(result.is_empty());
    }

    #[test]
    fn stale_multiple_batches() {
        let tasks = vec![
            make_task("1", 1, TaskStatus::InProgress),
            make_task("2", 2, TaskStatus::Completed),
            make_task("3", 3, TaskStatus::Pending),
        ];
        let batches = vec![
            make_batch(1, BatchStatus::Active, 5),
            make_batch(2, BatchStatus::Active, 5), // all completed → skipped
            make_batch(3, BatchStatus::Active, 5),
        ];
        let result = detect_stale_batches(&tasks, &batches, 3);
        assert_eq!(result.len(), 2);
        let ids: Vec<u64> = result.iter().map(|r| r.batch_id).collect();
        assert!(ids.contains(&1));
        assert!(ids.contains(&3));
    }

    #[test]
    fn stale_exactly_at_threshold_boundary() {
        let tasks = vec![make_task("1", 1, TaskStatus::InProgress)];
        // threshold = 1, silence_turns = 1 → exactly at boundary
        let batches = vec![make_batch(1, BatchStatus::Active, 1)];
        let result = detect_stale_batches(&tasks, &batches, 1);
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn stale_ignores_deleted_tasks() {
        let tasks = vec![make_task("1", 1, TaskStatus::Deleted)];
        let batches = vec![make_batch(1, BatchStatus::Active, 10)];
        let result = detect_stale_batches(&tasks, &batches, 5);
        assert!(result.is_empty());
    }

    #[test]
    fn stale_mixed_completed_and_incomplete() {
        let tasks = vec![
            make_task("1", 1, TaskStatus::Completed),
            make_task("2", 1, TaskStatus::InProgress),
            make_task("3", 1, TaskStatus::Deleted),
        ];
        let batches = vec![make_batch(1, BatchStatus::Active, 5)];
        let result = detect_stale_batches(&tasks, &batches, 3);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].incomplete_ids, vec!["2"]);
    }
}
