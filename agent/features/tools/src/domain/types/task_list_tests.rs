use super::{TaskListInput, TaskListResult};
use crate::domain::types::ToolSchema;
use task::{BatchCreateSpec, TaskCreateSpec, TaskPriority, TaskView};

fn task_view(subject: &str, description: &str, priority: TaskPriority) -> TaskView {
    let wiring = task::wire_task();
    let access = wiring.access();
    access
        .create_batch(BatchCreateSpec::try_new("批次".into()).unwrap(), 10)
        .unwrap();
    let task = access
        .create_task(
            TaskCreateSpec::try_new(subject.into(), description.into(), None, priority).unwrap(),
            11,
        )
        .unwrap()
        .value;
    TaskView::from(&task)
}

#[test]
fn task_list_input_schema_does_not_publish_session_id() {
    let schema = TaskListInput::data_schema();
    let properties = schema["properties"]
        .as_object()
        .expect("task list schema properties");
    assert!(!properties.contains_key("session_id"));
    assert!(!properties.contains_key("sessionId"));
    assert!(properties.contains_key("status"));
    assert!(properties.contains_key("priority"));
}

#[test]
fn task_list_result_serializes_each_task_with_the_stable_task_view_wire() {
    let value = serde_json::to_value(TaskListResult {
        tasks: vec![task_view("核验列表", "保持既有 wire", TaskPriority::Urgent)],
    })
    .expect("serialize task list result");

    assert_eq!(
        value,
        serde_json::json!({
            "tasks": [{
                "id": "1",
                "subject": "核验列表",
                "description": "保持既有 wire",
                "status": "pending",
                "blocked_by": [],
                "priority": "urgent",
                "created_at": 11,
                "updated_at": 11,
                "session_id": null,
                "batch": 1
            }]
        })
    );
}

#[test]
fn task_list_result_schema_uses_the_task_owned_view_without_legacy_owner() {
    let schema = TaskListResult::data_schema();
    let task_properties = schema["properties"]["tasks"]["items"]["properties"]
        .as_object()
        .expect("task list result task properties");
    assert_eq!(
        task_properties.keys().cloned().collect::<Vec<_>>(),
        vec![
            "batch",
            "blocked_by",
            "created_at",
            "description",
            "id",
            "priority",
            "session_id",
            "status",
            "subject",
            "updated_at",
        ]
    );
    assert!(!task_properties.contains_key("owner"));
}
