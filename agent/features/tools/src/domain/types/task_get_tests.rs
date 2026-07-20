use super::TaskGetResult;
use crate::domain::types::ToolSchema;
use task::{BatchCreateSpec, TaskCreateSpec, TaskPriority};

#[test]
fn task_get_result_uses_task_owned_view_without_legacy_owner() {
    let wiring = task::wire_task();
    let access = wiring.access();
    let batch = access
        .create_batch(BatchCreateSpec::try_new("批次".into()).unwrap(), 10)
        .unwrap()
        .value;
    let task = access
        .create_task(
            TaskCreateSpec::try_new(
                "核验输出".into(),
                "保持既有 wire".into(),
                None,
                TaskPriority::High,
            )
            .unwrap(),
            11,
        )
        .unwrap()
        .value;

    let value = serde_json::to_value(TaskGetResult {
        task: task::TaskView::from(&task),
    })
    .expect("serialize task result");
    assert_eq!(
        value,
        serde_json::json!({
            "task": {
                "id": "1",
                "subject": "核验输出",
                "description": "保持既有 wire",
                "status": "pending",
                "blocked_by": [],
                "priority": "high",
                "created_at": 11,
                "updated_at": 11,
                "session_id": null,
                "batch": batch.id().get()
            }
        })
    );
}

#[test]
fn task_get_result_schema_uses_the_task_owned_view_without_legacy_owner() {
    let schema = TaskGetResult::data_schema();
    let task_properties = schema["properties"]["task"]["properties"]
        .as_object()
        .expect("task get result properties");
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
