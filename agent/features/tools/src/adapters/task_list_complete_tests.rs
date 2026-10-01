use super::*;

fn test_ctx() -> ToolExecutionContext {
    crate::domain::test_support::TestToolExecutionContextBuilder::new(std::path::PathBuf::from("."))
        .build()
}

fn create_batch(access: &dyn task::TaskAccess) -> task::BatchData {
    access
        .create_batch(
            task::BatchCreateSpecData::try_new("当前请求".to_string()).unwrap(),
            1,
        )
        .unwrap()
        .value
}

#[tokio::test]
async fn test_task_list_complete_success_archives_current_batch() {
    let access = Arc::new(task::TaskStore::new());
    let access: Arc<dyn task::TaskAccess> = access.clone();
    let batch = create_batch(access.as_ref());
    let tool = TaskListCompleteTool {
        access: access.clone(),
    };

    let result = tool.call(serde_json::json!({}), &test_ctx()).await;

    assert!(!result.is_error, "{}", result.text);
    assert_eq!(result.data.unwrap().batch_id, batch.id().to_string());
    assert_eq!(
        access.list_batch_snapshots()[0].batch().status(),
        task::BatchStatusData::Archived
    );
}

#[tokio::test]
async fn test_task_list_complete_without_active_list_errors() {
    let access: Arc<dyn task::TaskAccess> = Arc::new(task::TaskStore::new());
    let tool = TaskListCompleteTool { access };

    let result = tool.call(serde_json::json!({}), &test_ctx()).await;

    assert!(result.is_error);
    assert!(result.text.contains("no active task list"));
}

#[tokio::test]
async fn test_task_list_complete_keeps_task_batch() {
    let access = Arc::new(task::TaskStore::new());
    let access: Arc<dyn task::TaskAccess> = access.clone();
    let batch = create_batch(access.as_ref());
    let created = access
        .create_task(
            task::TaskCreateSpecData::try_new(
                "任务".to_string(),
                "描述".to_string(),
                None,
                task::TaskPriorityData::Normal,
            )
            .unwrap(),
            2,
        )
        .unwrap()
        .value;
    let tool = TaskListCompleteTool {
        access: access.clone(),
    };

    let result = tool.call(serde_json::json!({}), &test_ctx()).await;

    assert!(!result.is_error, "{}", result.text);
    assert_eq!(access.get(created.id()).unwrap().batch(), batch.id());
}
