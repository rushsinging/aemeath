use super::sdk_event_mapper::map_stream_event;
use crate::application::loop_engine::chat::RuntimeStreamEvent;

#[test]
fn task_state_changed_mapping_preserves_complete_structured_state() {
    let expected = sdk::TaskStateView {
        session_id: "session-a".to_owned(),
        revision: 42,
        current_batch: Some(sdk::TaskBatchView {
            id: 7,
            summary: Some("ship".to_owned()),
            status: sdk::TaskBatchStatusView::Active,
        }),
        total: 3,
        completed: 1,
        in_progress: 1,
        items: vec![sdk::TaskItemView {
            id: 11,
            sequence: 3,
            subject: "verify".to_owned(),
            status: sdk::TaskItemStatusView::Pending,
            priority: sdk::TaskPriorityView::High,
            blocked_by_sequences: vec![2],
        }],
        hidden_count: 2,
    };

    let mapped = map_stream_event(RuntimeStreamEvent::TaskStateChanged {
        state: Box::new(expected.clone()),
    });

    match mapped {
        sdk::ChatEvent::TaskStateChanged { state } => assert_eq!(*state, expected),
        other => panic!("unexpected event: {other:?}"),
    }
}
