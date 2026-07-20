use super::{Batch, BatchId, BatchStatus, Task, TaskId, TaskStatus};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterruptedBatchInfo {
    pub batch_id: BatchId,
    pub incomplete_count: usize,
    pub incomplete_ids: Vec<TaskId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaleBatchInfo {
    pub batch_id: BatchId,
    pub silence_turns: u64,
    pub incomplete_ids: Vec<TaskId>,
}

pub fn detect_batch_all_completed(prev_batch: Option<BatchId>, tasks: &[Task]) -> Option<BatchId> {
    let batch_id = prev_batch?;
    let relevant: Vec<_> = tasks
        .iter()
        .filter(|task| task.batch() == batch_id && task.status() != TaskStatus::Deleted)
        .collect();
    (!relevant.is_empty()
        && relevant
            .iter()
            .all(|task| task.status() == TaskStatus::Completed))
    .then_some(batch_id)
}

pub fn detect_interrupted_batch(
    current_batch: BatchId,
    tasks: &[Task],
    batches: &[Batch],
    is_new_topic: bool,
) -> Option<InterruptedBatchInfo> {
    if !is_new_topic {
        return None;
    }
    let mut candidates: Vec<_> = batches
        .iter()
        .filter(|batch| batch.id() != current_batch && batch.status() == BatchStatus::Active)
        .collect();
    candidates.sort_unstable_by_key(|batch| batch.id());
    candidates.into_iter().find_map(|batch| {
        let mut incomplete_ids: Vec<_> = tasks
            .iter()
            .filter(|task| {
                task.batch() == batch.id()
                    && !matches!(task.status(), TaskStatus::Completed | TaskStatus::Deleted)
            })
            .map(Task::id)
            .collect();
        incomplete_ids.sort_unstable();
        (!incomplete_ids.is_empty()).then_some(InterruptedBatchInfo {
            batch_id: batch.id(),
            incomplete_count: incomplete_ids.len(),
            incomplete_ids,
        })
    })
}

pub fn detect_stale_batches(
    tasks: &[Task],
    batches: &[Batch],
    threshold: u64,
) -> Vec<StaleBatchInfo> {
    let mut result: Vec<_> = batches
        .iter()
        .filter(|batch| batch.status() == BatchStatus::Active && batch.silence_turns() >= threshold)
        .filter_map(|batch| {
            let mut incomplete_ids: Vec<_> = tasks
                .iter()
                .filter(|task| {
                    task.batch() == batch.id()
                        && !matches!(task.status(), TaskStatus::Completed | TaskStatus::Deleted)
                })
                .map(Task::id)
                .collect();
            incomplete_ids.sort_unstable();
            (!incomplete_ids.is_empty()).then_some(StaleBatchInfo {
                batch_id: batch.id(),
                silence_turns: batch.silence_turns(),
                incomplete_ids,
            })
        })
        .collect();
    result.sort_unstable_by_key(|info| info.batch_id);
    result
}

#[cfg(test)]
#[path = "lifecycle_tests.rs"]
mod tests;
