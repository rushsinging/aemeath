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
fn stale_threshold_boundaries_and_results_are_stable() {
    let tasks = vec![
        task(4, 2, TaskStatus::Pending),
        task(2, 2, TaskStatus::InProgress),
        task(3, 1, TaskStatus::Pending),
        task(1, 3, TaskStatus::Completed),
    ];
    let batches = vec![
        batch(2, BatchStatus::Active, 4),
        batch(3, BatchStatus::Active, 10),
        batch(1, BatchStatus::Active, 2),
        batch(4, BatchStatus::Archived, 10),
    ];

    let stale = detect_stale_batches(&tasks, &batches, 3);

    assert_eq!(stale.len(), 1);
    assert_eq!(stale[0].batch_id, BatchId::new(2));
    assert_eq!(
        stale[0].incomplete_ids,
        vec![TaskId::new(2), TaskId::new(4)]
    );
    let at_threshold = detect_stale_batches(&tasks, &[batch(1, BatchStatus::Active, 3)], 3);
    assert_eq!(at_threshold[0].batch_id, BatchId::new(1));
}

#[test]
fn interrupted_selection_is_independent_of_input_order() {
    let tasks = vec![
        task(3, 3, TaskStatus::Pending),
        task(1, 1, TaskStatus::Pending),
        task(2, 1, TaskStatus::InProgress),
    ];
    let forward = vec![
        batch(3, BatchStatus::Active, 0),
        batch(1, BatchStatus::Active, 0),
    ];
    let reverse = vec![
        batch(1, BatchStatus::Active, 0),
        batch(3, BatchStatus::Active, 0),
    ];
    let expected = InterruptedBatchInfo {
        batch_id: BatchId::new(1),
        incomplete_count: 2,
        incomplete_ids: vec![TaskId::new(1), TaskId::new(2)],
    };

    assert_eq!(
        detect_interrupted_batch(BatchId::new(9), &tasks, &forward, true),
        Some(expected.clone())
    );
    assert_eq!(
        detect_interrupted_batch(BatchId::new(9), &tasks, &reverse, true),
        Some(expected)
    );
}

#[test]
fn constructors_keep_specs_private_and_create_pending_entities() {
    let task = Task::create(
        TaskId::new(1),
        BatchId::new(1),
        TaskCreateSpec::try_new("任务".into(), "描述".into(), None, TaskPriority::Normal).unwrap(),
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
