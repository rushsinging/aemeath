use super::*;

fn test_ctx() -> ToolExecutionContext {
    crate::domain::test_support::TestToolExecutionContextBuilder::new(std::path::PathBuf::from("."))
        .build()
}

#[tokio::test]
async fn test_task_list_create_success_uses_summary_only() {
    let access = Arc::new(task::TaskStore::new());
    let access: Arc<dyn task::TaskAccess> = access.clone();
    let tool = TaskListCreateTool {
        access: access.clone(),
    };

    let result = tool
        .call(
            serde_json::json!({"subject": "legacy display", "summary": "修复 task 状态"}),
            &test_ctx(),
        )
        .await;

    assert!(!result.is_error, "{}", result.text);
    assert!(result.text.contains("Subject: legacy display"));
    let snapshots = access.list_batch_snapshots();
    assert_eq!(snapshots.len(), 1);
    assert_eq!(snapshots[0].batch().summary(), Some("修复 task 状态"));
    assert_eq!(
        result.data.unwrap().batch_id,
        snapshots[0].batch().id().to_string()
    );
}

#[tokio::test]
async fn test_task_list_create_missing_summary_errors() {
    let access: Arc<dyn task::TaskAccess> = Arc::new(task::TaskStore::new());
    let tool = TaskListCreateTool { access };

    let result = tool
        .call(serde_json::json!({"subject": "修复 bug"}), &test_ctx())
        .await;

    assert!(result.is_error);
    assert!(
        result.text.contains("summary") || result.text.contains("摘要"),
        "{}",
        result.text
    );
}

#[tokio::test]
async fn test_task_list_create_allows_task_create_membership_by_batch() {
    let access = Arc::new(task::TaskStore::new());
    let access: Arc<dyn task::TaskAccess> = access.clone();
    let tool = TaskListCreateTool {
        access: access.clone(),
    };

    let result = tool
        .call(
            serde_json::json!({"subject": "当前", "summary": "当前请求"}),
            &test_ctx(),
        )
        .await;
    assert!(!result.is_error, "{}", result.text);
    let task = access
        .create_task(
            task::TaskCreateSpecData::try_new(
                "任务".to_string(),
                "描述".to_string(),
                None,
                task::TaskPriorityData::Normal,
            )
            .unwrap(),
            1,
        )
        .unwrap()
        .value;

    assert_eq!(task.batch().to_string(), result.data.unwrap().batch_id);
}

#[test]
fn test_task_list_create_timeout_is_short_for_memory_only_tool() {
    let access: Arc<dyn task::TaskAccess> = Arc::new(task::TaskStore::new());
    let tool = TaskListCreateTool { access };

    assert_eq!(tool.timeout_secs(), 5);
}
