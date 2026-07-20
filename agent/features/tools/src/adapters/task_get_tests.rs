use super::*;

fn test_ctx() -> ToolExecutionContext {
    crate::domain::test_support::TestToolExecutionContextBuilder::new(std::path::PathBuf::from("."))
        .build()
}

fn seeded_access() -> Arc<dyn task::TaskAccess> {
    let access: Arc<dyn task::TaskAccess> = Arc::new(task::TaskStore::new());
    access
        .create_batch(task::BatchCreateSpec::try_new("batch".into()).unwrap(), 1)
        .unwrap();
    access
        .create_task(
            task::TaskCreateSpec::try_new(
                "任务".into(),
                "描述".into(),
                None,
                task::TaskPriority::Normal,
            )
            .unwrap(),
            2,
        )
        .unwrap();
    access
}

#[tokio::test]
async fn task_get_returns_task_owned_view_for_live_task() {
    let tool = TaskGetTool {
        access: seeded_access(),
    };

    let result = tool
        .call(serde_json::json!({"task_id": "1"}), &test_ctx())
        .await;

    assert!(!result.is_error, "{}", result.text);
    let value = serde_json::to_value(result.data.unwrap()).expect("serialize task result");
    assert_eq!(value["task"]["id"], "1");
}

#[tokio::test]
async fn task_get_hides_deleted_task() {
    let access = seeded_access();
    access.delete(task::TaskId::new(1), 3).unwrap();
    let tool = TaskGetTool { access };

    let result = tool
        .call(serde_json::json!({"task_id": "1"}), &test_ctx())
        .await;

    assert!(result.is_error);
    assert!(result.text.contains("Task not found"));
}

#[tokio::test]
async fn task_get_rejects_zero_id_before_task_access() {
    let tool = TaskGetTool {
        access: seeded_access(),
    };

    let result = tool
        .call(serde_json::json!({"task_id": "0"}), &test_ctx())
        .await;

    assert!(result.is_error);
    assert!(result.text.contains("non-zero decimal number"));
}
