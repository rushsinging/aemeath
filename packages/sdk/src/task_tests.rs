use super::*;

fn sample_task_state() -> TaskStateView {
    TaskStateView {
        session_id: "session-a".to_owned(),
        revision: 42,
        current_batch: Some(TaskBatchView {
            id: 7,
            summary: Some("ship".to_owned()),
            status: TaskBatchStatusView::Active,
        }),
        total: 3,
        completed: 1,
        in_progress: 1,
        items: vec![
            TaskItemView {
                id: 11,
                sequence: 1,
                subject: "done".to_owned(),
                status: TaskItemStatusView::Completed,
                priority: TaskPriorityView::Normal,
                blocked_by_sequences: Vec::new(),
            },
            TaskItemView {
                id: 12,
                sequence: 2,
                subject: "work".to_owned(),
                status: TaskItemStatusView::InProgress,
                priority: TaskPriorityView::High,
                blocked_by_sequences: Vec::new(),
            },
            TaskItemView {
                id: 13,
                sequence: 3,
                subject: "later".to_owned(),
                status: TaskItemStatusView::Pending,
                priority: TaskPriorityView::Low,
                blocked_by_sequences: vec![2],
            },
        ],
        hidden_count: 0,
    }
}

#[test]
fn task_state_view_round_trips_without_field_loss() {
    let expected = sample_task_state();

    let encoded = serde_json::to_value(&expected).unwrap();
    let decoded: TaskStateView = serde_json::from_value(encoded).unwrap();

    assert_eq!(decoded, expected);
}

#[test]
fn empty_task_state_keeps_session_and_revision() {
    let state = TaskStateView::empty("session-b", 3);

    assert_eq!(state.session_id, "session-b");
    assert_eq!(state.revision, 3);
    assert!(state.current_batch.is_none());
    assert!(state.items.is_empty());
}

#[test]
fn wire_components_include_task_state() {
    let document = crate::wire::components_document();
    let definitions = document["$defs"].as_object().unwrap();

    for name in [
        "TaskStateView",
        "TaskBatchView",
        "TaskBatchStatusView",
        "TaskItemView",
        "TaskItemStatusView",
        "TaskPriorityView",
    ] {
        assert!(definitions.contains_key(name), "missing schema: {name}");
    }
}
