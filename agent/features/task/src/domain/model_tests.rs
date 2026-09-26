use crate::{
    domain::TaskCommandError, BatchCreateSpecData, BatchData, BatchIdData, BatchStatusData,
    TaskCreateSpecData, TaskData, TaskEventData, TaskIdData, TaskPriorityData, TaskRevisionData,
    TaskStatusData,
};

#[test]
fn task_id_tool_input_rejects_zero_and_malformed_values() {
    assert_eq!(
        TaskIdData::parse_tool_input("1").unwrap(),
        TaskIdData::new(1)
    );
    for value in ["", "0", "-1", " 1", "001", "18446744073709551616"] {
        assert!(TaskIdData::parse_tool_input(value).is_err(), "{value}");
    }
}

#[test]
fn ids_specs_and_priority_validate_values() {
    assert_eq!(TaskIdData::new(7).to_string(), "7");
    assert_eq!(BatchIdData::new(9).get(), 9);
    assert_eq!(TaskPriorityData::default(), TaskPriorityData::Normal);
    assert!(TaskPriorityData::Low < TaskPriorityData::Normal);
    assert!(TaskPriorityData::Normal < TaskPriorityData::High);
    assert!(TaskPriorityData::High < TaskPriorityData::Urgent);
    assert_eq!(
        TaskCreateSpecData::try_new(" ".into(), String::new(), None, TaskPriorityData::Normal),
        Err(TaskCommandError::InvalidTaskSubject.into())
    );
    assert_eq!(
        BatchCreateSpecData::try_new("\t".into()),
        Err(TaskCommandError::InvalidBatchSummary.into())
    );
}

#[test]
fn task_revision_is_an_ordered_numeric_value() {
    assert_eq!(TaskRevisionData::new(0).get(), 0);
    assert_eq!(TaskRevisionData::new(7).to_string(), "7");
    assert!(TaskRevisionData::new(2) > TaskRevisionData::new(1));
}

#[test]
fn local_domain_results_are_uncommitted_until_store_transaction() {
    let result = TaskData::create(
        TaskIdData::new(1),
        BatchIdData::new(1),
        1,
        TaskCreateSpecData::try_new("任务".into(), String::new(), None, TaskPriorityData::Normal)
            .unwrap(),
        1,
    );
    assert_eq!(result.revision(), None);
}

#[test]
fn task_create_and_local_fields_preserve_private_state() {
    let result = TaskData::create(
        TaskIdData::new(2),
        BatchIdData::new(3),
        1,
        TaskCreateSpecData::try_new(
            "任务".into(),
            "描述".into(),
            Some("进行中".into()),
            TaskPriorityData::High,
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
        vec![TaskEventData::TaskCreated {
            task_id: TaskIdData::new(2)
        }]
    );
}

#[test]
fn task_subject_and_description_updates_are_typed_and_idempotent() {
    let mut task = TaskData::create(
        TaskIdData::new(1),
        BatchIdData::new(1),
        1,
        TaskCreateSpecData::try_new(
            "原始标题".into(),
            "原始描述".into(),
            None,
            TaskPriorityData::Normal,
        )
        .unwrap(),
        10,
    )
    .value;

    let subject = task
        .set_subject("新标题".to_owned(), 11)
        .expect("非空标题应更新");
    assert_eq!(subject.value.subject(), "新标题");
    assert_eq!(
        subject.events,
        vec![TaskEventData::TaskSubjectChanged {
            task_id: TaskIdData::new(1),
        }]
    );

    let duplicate = task
        .set_subject("新标题".to_owned(), 12)
        .expect("重复标题应幂等成功");
    assert!(duplicate.events.is_empty());
    assert_eq!(duplicate.value.updated_at(), 11);

    let description = task.set_description("新描述".to_owned(), 13);
    assert_eq!(description.value.description(), "新描述");
    assert_eq!(
        description.events,
        vec![TaskEventData::TaskDescriptionChanged {
            task_id: TaskIdData::new(1),
        }]
    );

    assert_eq!(
        task.set_subject("   ".to_owned(), 14),
        Err(TaskCommandError::InvalidTaskSubject.into())
    );
    assert_eq!(task.subject(), "新标题");
    assert_eq!(task.updated_at(), 13);
}

#[test]
fn task_allows_only_documented_live_transitions() {
    for (from, to) in [
        (TaskStatusData::Pending, TaskStatusData::InProgress),
        (TaskStatusData::Pending, TaskStatusData::Completed),
        (TaskStatusData::InProgress, TaskStatusData::Pending),
        (TaskStatusData::InProgress, TaskStatusData::Completed),
    ] {
        let mut task = TaskData::with_status(TaskIdData::new(1), BatchIdData::new(1), from, 10);
        let result = task.transition_to(to, 20).unwrap();
        assert_eq!(result.value.status(), to);
        assert_eq!(result.value.updated_at(), 20);
        assert_eq!(
            result.events,
            vec![TaskEventData::TaskStatusChanged {
                task_id: TaskIdData::new(1),
                from,
                to,
            }]
        );
    }
}

#[test]
fn task_execution_timestamps_follow_status_transitions() {
    let mut task = TaskData::with_status(
        TaskIdData::new(1),
        BatchIdData::new(1),
        TaskStatusData::Pending,
        10,
    );
    task.transition_to(TaskStatusData::InProgress, 20).unwrap();
    assert_eq!(task.started_at(), Some(20));
    assert_eq!(task.completed_at(), None);

    task.transition_to(TaskStatusData::Pending, 30).unwrap();
    assert_eq!(
        task.started_at(),
        None,
        "回退到 pending 后不得保留执行时间戳，否则 snapshot 无法恢复"
    );
    task.transition_to(TaskStatusData::InProgress, 40).unwrap();
    assert_eq!(task.started_at(), Some(40));
    assert_eq!(task.completed_at(), None);

    task.transition_to(TaskStatusData::Completed, 50).unwrap();
    assert_eq!(task.started_at(), Some(40));
    assert_eq!(task.completed_at(), Some(50));

    let mut directly_completed = TaskData::with_status(
        TaskIdData::new(2),
        BatchIdData::new(1),
        TaskStatusData::Pending,
        10,
    );
    directly_completed
        .transition_to(TaskStatusData::Completed, 60)
        .unwrap();
    assert_eq!(directly_completed.started_at(), Some(60));
    assert_eq!(directly_completed.completed_at(), Some(60));
}

#[test]
fn task_illegal_transition_leaves_state_unchanged() {
    let mut task = TaskData::with_status(
        TaskIdData::new(1),
        BatchIdData::new(1),
        TaskStatusData::Completed,
        10,
    );
    assert_eq!(
        task.transition_to(TaskStatusData::Pending, 20),
        Err(TaskCommandError::IllegalTransition {
            from: TaskStatusData::Completed,
            to: TaskStatusData::Pending
        }
        .into())
    );
    assert_eq!(task.status(), TaskStatusData::Completed);
    assert_eq!(task.updated_at(), 10);
    assert_eq!(task.started_at(), Some(10));
    assert_eq!(task.completed_at(), Some(10));
    assert_eq!(
        task.transition_to(TaskStatusData::Deleted, 20),
        Err(TaskCommandError::DeletedOnlyViaDelete.into())
    );
}

#[test]
fn task_local_mutations_update_timestamp_only_when_state_changes() {
    let mut task = TaskData::with_status(
        TaskIdData::new(1),
        BatchIdData::new(1),
        TaskStatusData::Pending,
        10,
    );

    task.set_priority(TaskPriorityData::Urgent, 20);
    assert_eq!(task.priority(), TaskPriorityData::Urgent);
    assert_eq!(task.updated_at(), 20);
    task.set_priority(TaskPriorityData::Urgent, 25);
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
        (BatchStatusData::Active, BatchStatusData::Paused),
        (BatchStatusData::Active, BatchStatusData::Archived),
        (BatchStatusData::Paused, BatchStatusData::Active),
        (BatchStatusData::Paused, BatchStatusData::Archived),
        (BatchStatusData::Archived, BatchStatusData::Archived),
    ] {
        let mut batch = BatchData::with_status(BatchIdData::new(1), from, 0);
        batch.transition_to(to).unwrap();
        assert_eq!(batch.status(), to);
    }
    let mut archived = BatchData::with_status(BatchIdData::new(1), BatchStatusData::Archived, 0);
    assert!(matches!(
        archived.transition_to(BatchStatusData::Active),
        Err(ref error) if error.message().contains("不允许从")
    ));
    assert_eq!(archived.status(), BatchStatusData::Archived);

    for status in [BatchStatusData::Active, BatchStatusData::Paused] {
        let mut batch = BatchData::with_status(BatchIdData::new(1), status, 0);
        assert!(matches!(
            batch.transition_to(status),
            Err(ref error) if error.message().contains("不允许从")
        ));
        assert_eq!(batch.status(), status);
    }
}
