use super::*;

fn test_ctx() -> ToolExecutionContext {
    crate::domain::test_support::TestToolExecutionContextBuilder::new(std::path::PathBuf::from("."))
        .build()
}

#[tokio::test]
async fn task_create_uses_task_access_and_active_batch() {
    let store = Arc::new(task::TaskStore::new());
    let access: Arc<dyn task::TaskAccess> = store.clone();
    let batch = access
        .create_batch(task::BatchCreateSpec::try_new("batch".into()).unwrap(), 1)
        .unwrap();
    let tool = TaskCreateTool { access };

    let result = tool
        .call(
            serde_json::json!({
                "subject": "测试任务",
                "description": "描述",
                "priority": "high"
            }),
            &test_ctx(),
        )
        .await;

    assert!(!result.is_error, "{}", result.text);
    let tasks = store.list();
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].batch(), batch.value.id());
    assert_eq!(tasks[0].priority(), task::TaskPriority::High);
    assert_eq!(tasks[0].session_id(), None);
}

#[tokio::test]
async fn task_create_without_active_batch_returns_typed_error() {
    let store = Arc::new(task::TaskStore::new());
    let access: Arc<dyn task::TaskAccess> = store.clone();
    let tool = TaskCreateTool { access };

    let result = tool
        .call(
            serde_json::json!({"subject": "任务", "description": "描述"}),
            &test_ctx(),
        )
        .await;

    assert!(result.is_error);
    assert!(result.text.contains("active"), "{}", result.text);
    assert!(store.list().is_empty());
    assert!(store.list_batches().is_empty());
}
