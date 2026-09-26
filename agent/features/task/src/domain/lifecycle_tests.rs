use super::*;
use crate::{BatchCreateSpecData, TaskCreateSpecData, TaskPriorityData};

fn task(id: u64, batch: u64, status: TaskStatusData) -> TaskData {
    TaskData::with_status(TaskIdData::new(id), BatchIdData::new(batch), status, 0)
}

fn batch(id: u64, status: BatchStatusData, silence: u64) -> BatchData {
    BatchData::with_status(BatchIdData::new(id), status, silence)
}

#[test]
fn all_completed_ignores_deleted_and_other_batches() {
    let tasks = vec![
        task(1, 1, TaskStatusData::Completed),
        task(2, 1, TaskStatusData::Deleted),
        task(3, 2, TaskStatusData::Pending),
    ];

    assert_eq!(
        detect_batch_all_completed(Some(BatchIdData::new(1)), &tasks),
        Some(BatchIdData::new(1))
    );
}

#[test]
fn interrupted_reports_incomplete_typed_ids() {
    let tasks = vec![
        task(1, 1, TaskStatusData::Completed),
        task(2, 1, TaskStatusData::InProgress),
    ];
    let batches = vec![batch(1, BatchStatusData::Active, 0)];

    let info = detect_interrupted_batch(BatchIdData::new(2), &tasks, &batches, true).unwrap();

    assert_eq!(info.incomplete_ids, vec![TaskIdData::new(2)]);
}

#[test]
fn stale_respects_threshold_and_active_status() {
    let tasks = vec![
        task(1, 1, TaskStatusData::Pending),
        task(2, 2, TaskStatusData::Pending),
    ];
    let batches = vec![
        batch(1, BatchStatusData::Active, 3),
        batch(2, BatchStatusData::Paused, 5),
    ];

    let stale = detect_stale_batches(&tasks, &batches, 3);

    assert_eq!(stale.len(), 1);
    assert_eq!(stale[0].batch_id, BatchIdData::new(1));
}

#[test]
fn stale_threshold_boundaries_and_results_are_stable() {
    let tasks = vec![
        task(4, 2, TaskStatusData::Pending),
        task(2, 2, TaskStatusData::InProgress),
        task(3, 1, TaskStatusData::Pending),
        task(1, 3, TaskStatusData::Completed),
    ];
    let batches = vec![
        batch(2, BatchStatusData::Active, 4),
        batch(3, BatchStatusData::Active, 10),
        batch(1, BatchStatusData::Active, 2),
        batch(4, BatchStatusData::Archived, 10),
    ];

    let stale = detect_stale_batches(&tasks, &batches, 3);

    assert_eq!(stale.len(), 1);
    assert_eq!(stale[0].batch_id, BatchIdData::new(2));
    assert_eq!(
        stale[0].incomplete_ids,
        vec![TaskIdData::new(2), TaskIdData::new(4)]
    );
    let at_threshold = detect_stale_batches(&tasks, &[batch(1, BatchStatusData::Active, 3)], 3);
    assert_eq!(at_threshold[0].batch_id, BatchIdData::new(1));
}

#[test]
fn interrupted_selection_is_independent_of_input_order() {
    let tasks = vec![
        task(3, 3, TaskStatusData::Pending),
        task(1, 1, TaskStatusData::Pending),
        task(2, 1, TaskStatusData::InProgress),
    ];
    let forward = vec![
        batch(3, BatchStatusData::Active, 0),
        batch(1, BatchStatusData::Active, 0),
    ];
    let reverse = vec![
        batch(1, BatchStatusData::Active, 0),
        batch(3, BatchStatusData::Active, 0),
    ];
    let expected = InterruptedBatchInfoData {
        batch_id: BatchIdData::new(1),
        incomplete_count: 2,
        incomplete_ids: vec![TaskIdData::new(1), TaskIdData::new(2)],
    };

    assert_eq!(
        detect_interrupted_batch(BatchIdData::new(9), &tasks, &forward, true),
        Some(expected.clone())
    );
    assert_eq!(
        detect_interrupted_batch(BatchIdData::new(9), &tasks, &reverse, true),
        Some(expected)
    );
}

#[test]
fn constructors_keep_specs_private_and_create_pending_entities() {
    let task = TaskData::create(
        TaskIdData::new(1),
        BatchIdData::new(1),
        1,
        TaskCreateSpecData::try_new("任务".into(), "描述".into(), None, TaskPriorityData::Normal)
            .unwrap(),
        0,
    )
    .value;
    let batch = BatchData::create(
        BatchIdData::new(1),
        BatchCreateSpecData::try_new("批次".into()).unwrap(),
        0,
    );

    assert_eq!(task.status(), TaskStatusData::Pending);
    assert_eq!(batch.status(), BatchStatusData::Active);
}
