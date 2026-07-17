use crate::{
    Batch, BatchCreateSpec, BatchId, BatchStatus, Task, TaskCommandError, TaskCreateSpec,
    TaskEvent, TaskId, TaskPriority, TaskStatus,
};

#[test]
fn ids_specs_and_priority_validate_values() {
    assert_eq!(TaskId::new(7).to_string(), "7");
    assert_eq!(BatchId::new(9).get(), 9);
    assert_eq!(TaskPriority::default(), TaskPriority::Normal);
    assert!(TaskPriority::Low < TaskPriority::Normal);
    assert!(TaskPriority::Normal < TaskPriority::High);
    assert!(TaskPriority::High < TaskPriority::Urgent);
    assert_eq!(
        TaskCreateSpec::try_new(" ".into(), String::new(), None, TaskPriority::Normal),
        Err(TaskCommandError::InvalidTaskSubject)
    );
    assert_eq!(
        BatchCreateSpec::try_new("\t".into()),
        Err(TaskCommandError::InvalidBatchSummary)
    );
}

#[test]
fn task_create_and_local_fields_preserve_private_state() {
    let result = Task::create(
        TaskId::new(2),
        BatchId::new(3),
        TaskCreateSpec::try_new(
            "任务".into(),
            "描述".into(),
            Some("进行中".into()),
            TaskPriority::High,
        )
        .unwrap(),
        10,
    );
    assert_eq!(result.value.subject(), "任务");
    assert_eq!(result.value.description(), "描述");
    assert_eq!(result.value.active_form(), Some("进行中"));
    assert_eq!(result.value.session_id(), None);
    assert!(result.value.tags().is_empty());
    assert_eq!(result.value.started_at(), None);
    assert_eq!(result.value.completed_at(), None);
    assert_eq!(
        result.events,
        vec![TaskEvent::TaskCreated {
            task_id: TaskId::new(2)
        }]
    );
}

#[test]
fn task_allows_only_documented_live_transitions() {
    for (from, to) in [
        (TaskStatus::Pending, TaskStatus::InProgress),
        (TaskStatus::Pending, TaskStatus::Completed),
        (TaskStatus::InProgress, TaskStatus::Pending),
        (TaskStatus::InProgress, TaskStatus::Completed),
    ] {
        let mut task = Task::with_status(TaskId::new(1), BatchId::new(1), from, 10);
        let result = task.transition_to(to, 20).unwrap();
        assert_eq!(result.value.status(), to);
        assert_eq!(result.value.updated_at(), 20);
        assert_eq!(
            result.events,
            vec![TaskEvent::TaskStatusChanged {
                task_id: TaskId::new(1),
                from,
                to,
            }]
        );
    }
}

#[test]
fn task_execution_timestamps_follow_status_transitions() {
    let mut task = Task::with_status(TaskId::new(1), BatchId::new(1), TaskStatus::Pending, 10);
    task.transition_to(TaskStatus::InProgress, 20).unwrap();
    assert_eq!(task.started_at(), Some(20));
    assert_eq!(task.completed_at(), None);

    task.transition_to(TaskStatus::Pending, 30).unwrap();
    task.transition_to(TaskStatus::InProgress, 40).unwrap();
    assert_eq!(task.started_at(), Some(20));
    assert_eq!(task.completed_at(), None);

    task.transition_to(TaskStatus::Completed, 50).unwrap();
    assert_eq!(task.started_at(), Some(20));
    assert_eq!(task.completed_at(), Some(50));

    let mut directly_completed =
        Task::with_status(TaskId::new(2), BatchId::new(1), TaskStatus::Pending, 10);
    directly_completed
        .transition_to(TaskStatus::Completed, 60)
        .unwrap();
    assert_eq!(directly_completed.started_at(), Some(60));
    assert_eq!(directly_completed.completed_at(), Some(60));
}

#[test]
fn task_illegal_transition_leaves_state_unchanged() {
    let mut task = Task::with_status(TaskId::new(1), BatchId::new(1), TaskStatus::Completed, 10);
    assert_eq!(
        task.transition_to(TaskStatus::Pending, 20),
        Err(TaskCommandError::IllegalTransition {
            from: TaskStatus::Completed,
            to: TaskStatus::Pending
        })
    );
    assert_eq!(task.status(), TaskStatus::Completed);
    assert_eq!(task.updated_at(), 10);
    assert_eq!(task.started_at(), Some(10));
    assert_eq!(task.completed_at(), Some(10));
    assert_eq!(
        task.transition_to(TaskStatus::Deleted, 20),
        Err(TaskCommandError::DeletedOnlyViaDelete)
    );
}

#[test]
fn task_local_mutations_update_timestamp_only_when_state_changes() {
    let mut task = Task::with_status(TaskId::new(1), BatchId::new(1), TaskStatus::Pending, 10);

    task.set_priority(TaskPriority::Urgent, 20);
    assert_eq!(task.priority(), TaskPriority::Urgent);
    assert_eq!(task.updated_at(), 20);

    task.add_tag("backend".into(), 30);
    assert_eq!(task.tags(), &["backend".to_string()]);
    assert_eq!(task.updated_at(), 30);
    task.add_tag("backend".into(), 40);
    assert_eq!(task.tags(), &["backend".to_string()]);
    assert_eq!(task.updated_at(), 30);

    task.remove_tag("missing", 50);
    assert_eq!(task.updated_at(), 30);
    task.remove_tag("backend", 60);
    assert!(task.tags().is_empty());
    assert_eq!(task.updated_at(), 60);
}

#[test]
fn batch_allows_only_documented_local_transitions() {
    for (from, to) in [
        (BatchStatus::Active, BatchStatus::Paused),
        (BatchStatus::Active, BatchStatus::Archived),
        (BatchStatus::Paused, BatchStatus::Active),
        (BatchStatus::Paused, BatchStatus::Archived),
        (BatchStatus::Archived, BatchStatus::Archived),
    ] {
        let mut batch = Batch::with_status(BatchId::new(1), from, 0);
        batch.transition_to(to).unwrap();
        assert_eq!(batch.status(), to);
    }
    let mut archived = Batch::with_status(BatchId::new(1), BatchStatus::Archived, 0);
    assert!(matches!(
        archived.transition_to(BatchStatus::Active),
        Err(TaskCommandError::IllegalBatchTransition { .. })
    ));
    assert_eq!(archived.status(), BatchStatus::Archived);

    for status in [BatchStatus::Active, BatchStatus::Paused] {
        let mut batch = Batch::with_status(BatchId::new(1), status, 0);
        assert!(matches!(
            batch.transition_to(status),
            Err(TaskCommandError::IllegalBatchTransition { .. })
        ));
        assert_eq!(batch.status(), status);
    }
}
