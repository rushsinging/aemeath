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
    batches
        .iter()
        .filter(|batch| batch.id() != current_batch && batch.status() == BatchStatus::Active)
        .find_map(|batch| {
            let incomplete_ids: Vec<_> = tasks
                .iter()
                .filter(|task| {
                    task.batch() == batch.id()
                        && !matches!(task.status(), TaskStatus::Completed | TaskStatus::Deleted)
                })
                .map(Task::id)
                .collect();
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
    batches
        .iter()
        .filter(|batch| batch.status() == BatchStatus::Active && batch.silence_turns() >= threshold)
        .filter_map(|batch| {
            let incomplete_ids: Vec<_> = tasks
                .iter()
                .filter(|task| {
                    task.batch() == batch.id()
                        && !matches!(task.status(), TaskStatus::Completed | TaskStatus::Deleted)
                })
                .map(Task::id)
                .collect();
            (!incomplete_ids.is_empty()).then_some(StaleBatchInfo {
                batch_id: batch.id(),
                silence_turns: batch.silence_turns(),
                incomplete_ids,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BatchCreateSpec, TaskCreateSpec, TaskPriority};

    fn task(id: u64, batch: u64, status: TaskStatus) -> Task {
        Task::with_status(TaskId::new(id), BatchId::new(batch), status, 0)
    }
    fn batch(id: u64, status: BatchStatus, silence: u64) -> Batch {
        Batch::with_status(BatchId::new(id), status, silence)
    }

    #[test]
    fn all_completed_ignores_deleted_and_other_batches() {
        let tasks = vec![
            task(1, 1, TaskStatus::Completed),
            task(2, 1, TaskStatus::Deleted),
            task(3, 2, TaskStatus::Pending),
        ];
        assert_eq!(
            detect_batch_all_completed(Some(BatchId::new(1)), &tasks),
            Some(BatchId::new(1))
        );
    }

    #[test]
    fn interrupted_reports_incomplete_typed_ids() {
        let tasks = vec![
            task(1, 1, TaskStatus::Completed),
            task(2, 1, TaskStatus::InProgress),
        ];
        let batches = vec![batch(1, BatchStatus::Active, 0)];
        let info = detect_interrupted_batch(BatchId::new(2), &tasks, &batches, true).unwrap();
        assert_eq!(info.incomplete_ids, vec![TaskId::new(2)]);
    }

    #[test]
    fn stale_respects_threshold_and_active_status() {
        let tasks = vec![
            task(1, 1, TaskStatus::Pending),
            task(2, 2, TaskStatus::Pending),
        ];
        let batches = vec![
            batch(1, BatchStatus::Active, 3),
            batch(2, BatchStatus::Paused, 5),
        ];
        let stale = detect_stale_batches(&tasks, &batches, 3);
        assert_eq!(stale.len(), 1);
        assert_eq!(stale[0].batch_id, BatchId::new(1));
    }

    #[test]
    fn constructors_keep_specs_private_and_create_pending_entities() {
        let task = Task::create(
            TaskId::new(1),
            BatchId::new(1),
            TaskCreateSpec::try_new("任务".into(), "描述".into(), None, TaskPriority::Normal)
                .unwrap(),
            0,
        )
        .value;
        let batch = Batch::create(
            BatchId::new(1),
            BatchCreateSpec::try_new("批次".into()).unwrap(),
            0,
        );
        assert_eq!(task.status(), TaskStatus::Pending);
        assert_eq!(batch.status(), BatchStatus::Active);
    }
}
