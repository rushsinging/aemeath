use super::published_language::ToolSuccess;
use super::{CommittedTaskChange, ToolResult, TypedToolResult};
use crate::domain::types::task_create::TaskCreateResult;
use serde_json::json;
use task::{BatchCreateSpec, TaskAccess, TaskCreateSpec, TaskPriority, TaskStore};

fn committed_change() -> CommittedTaskChange {
    let store = TaskStore::new();
    store
        .create_batch(BatchCreateSpec::try_new("batch".to_owned()).unwrap(), 1)
        .unwrap();
    let result = store
        .create_task(
            TaskCreateSpec::try_new(
                "task".to_owned(),
                "description".to_owned(),
                None,
                TaskPriority::Normal,
            )
            .unwrap(),
            2,
        )
        .unwrap();
    CommittedTaskChange::from_command_result(&result).expect("fixture must commit")
}

#[test]
fn typed_result_preserves_runtime_task_change_without_changing_llm_data() {
    let change = committed_change();
    let result = TypedToolResult::success(
        "Task #1 created",
        TaskCreateResult {
            task_id: "1".to_owned(),
            display_id: "1".to_owned(),
            subject: "task".to_owned(),
            status: "pending".to_owned(),
            priority: "normal".to_owned(),
        },
    )
    .with_task_change(Some(change.clone()));

    assert_eq!(result.text, "Task #1 created");
    assert_eq!(result.task_change, Some(change));
    assert!(result.data.is_some());
}

#[test]
fn typed_adapter_and_legacy_mapping_preserve_change_and_hide_it_from_public_data() {
    let change = committed_change();
    let typed = TypedToolResult::success("ok", json!({"status": "pending"}))
        .with_task_change(Some(change.clone()));
    let typed_data = typed.data.clone();
    let legacy = ToolResult {
        text: typed.text,
        data: typed_data.clone().unwrap(),
        is_error: typed.is_error,
        error_kind: typed.error_kind,
        images: typed.images,
        task_change: typed.task_change,
    };

    assert_eq!(legacy.text, "ok");
    assert_eq!(legacy.data, typed_data.unwrap());
    assert_eq!(legacy.task_change, Some(change.clone()));

    let outcome = super::published_language::ToolOutcome::Success(ToolSuccess {
        content: vec![super::published_language::ContentBlock::text("ok")],
        data: Some(legacy.data.clone()),
        metadata: Default::default(),
        task_change: legacy.task_change.clone(),
    });
    match outcome {
        super::published_language::ToolOutcome::Success(success) => {
            assert_eq!(success.task_change, Some(change));
            assert_eq!(success.data, Some(json!({"status": "pending"})));
        }
        _ => panic!("expected success"),
    }
}
