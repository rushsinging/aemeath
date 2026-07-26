use super::*;

fn test_ctx() -> ToolExecutionContext {
    crate::domain::test_support::TestToolExecutionContextBuilder::new(std::path::PathBuf::from("."))
        .build()
}

#[tokio::test]
async fn task_lists_discovers_historical_and_current_batches() {
    let access: Arc<dyn task::TaskAccess> = Arc::new(task::TaskStore::new());
    access
        .create_batch(task::BatchCreateSpec::try_new("历史".into()).unwrap(), 1)
        .unwrap();
    access.archive_batch(task::BatchId::new(1)).unwrap();
    access
        .create_batch(task::BatchCreateSpec::try_new("当前".into()).unwrap(), 2)
        .unwrap();

    let result = TaskListsTool { access }
        .call(serde_json::json!({}), &test_ctx())
        .await;

    assert!(!result.is_error, "{}", result.text);
    let value = serde_json::to_value(result.data.unwrap()).unwrap();
    assert_eq!(value["task_lists"].as_array().unwrap().len(), 2);
    assert_eq!(value["task_lists"][0]["task_list"]["id"], "1");
    assert_eq!(value["task_lists"][0]["task_list"]["status"], "archived");
    assert_eq!(value["task_lists"][1]["task_list"]["status"], "active");
}

#[tokio::test]
async fn task_lists_filters_status_and_rejects_unknown_status() {
    let access: Arc<dyn task::TaskAccess> = Arc::new(task::TaskStore::new());
    access
        .create_batch(task::BatchCreateSpec::try_new("当前".into()).unwrap(), 1)
        .unwrap();
    let tool = TaskListsTool { access };

    let active = tool
        .call(serde_json::json!({"status": "active"}), &test_ctx())
        .await;
    assert_eq!(active.data.unwrap().task_lists.len(), 1);

    let invalid = tool
        .call(serde_json::json!({"status": "unknown"}), &test_ctx())
        .await;
    assert!(invalid.is_error);
}
