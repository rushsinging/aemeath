use super::{BatchData, BatchIdData, BatchStatusData, TaskData, TaskIdData, TaskStatusData};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterruptedBatchInfoData {
    pub batch_id: BatchIdData,
    pub incomplete_count: usize,
    pub incomplete_ids: Vec<TaskIdData>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaleBatchInfoData {
    pub batch_id: BatchIdData,
    pub silence_turns: u64,
    pub incomplete_ids: Vec<TaskIdData>,
}

pub fn detect_batch_all_completed(
    prev_batch: Option<BatchIdData>,
    tasks: &[TaskData],
) -> Option<BatchIdData> {
    let batch_id = prev_batch?;
    let relevant: Vec<_> = tasks
        .iter()
        .filter(|task| task.batch() == batch_id && task.status() != TaskStatusData::Deleted)
        .collect();
    (!relevant.is_empty()
        && relevant
            .iter()
            .all(|task| task.status() == TaskStatusData::Completed))
    .then_some(batch_id)
}

pub fn detect_interrupted_batch(
    current_batch: BatchIdData,
    tasks: &[TaskData],
    batches: &[BatchData],
    is_new_topic: bool,
) -> Option<InterruptedBatchInfoData> {
    if !is_new_topic {
        return None;
    }
    let mut candidates: Vec<_> = batches
        .iter()
        .filter(|batch| batch.id() != current_batch && batch.status() == BatchStatusData::Active)
        .collect();
    candidates.sort_unstable_by_key(|batch| batch.id());
    candidates.into_iter().find_map(|batch| {
        let mut incomplete_ids: Vec<_> = tasks
            .iter()
            .filter(|task| {
                task.batch() == batch.id()
                    && !matches!(
                        task.status(),
                        TaskStatusData::Completed | TaskStatusData::Deleted
                    )
            })
            .map(TaskData::id)
            .collect();
        incomplete_ids.sort_unstable();
        (!incomplete_ids.is_empty()).then_some(InterruptedBatchInfoData {
            batch_id: batch.id(),
            incomplete_count: incomplete_ids.len(),
            incomplete_ids,
        })
    })
}

pub fn detect_stale_batches(
    tasks: &[TaskData],
    batches: &[BatchData],
    threshold: u64,
) -> Vec<StaleBatchInfoData> {
    let mut result: Vec<_> = batches
        .iter()
        .filter(|batch| {
            batch.status() == BatchStatusData::Active && batch.silence_turns() >= threshold
        })
        .filter_map(|batch| {
            let mut incomplete_ids: Vec<_> = tasks
                .iter()
                .filter(|task| {
                    task.batch() == batch.id()
                        && !matches!(
                            task.status(),
                            TaskStatusData::Completed | TaskStatusData::Deleted
                        )
                })
                .map(TaskData::id)
                .collect();
            incomplete_ids.sort_unstable();
            (!incomplete_ids.is_empty()).then_some(StaleBatchInfoData {
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
