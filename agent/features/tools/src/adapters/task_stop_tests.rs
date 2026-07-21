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
                String::new(),
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
async fn task_stop_marks_pending_task_deleted() {
    let access = seeded_access();
    let tool = TaskStopTool {
        access: access.clone(),
    };

    let result = tool
        .call(serde_json::json!({"task_id": "1"}), &test_ctx())
        .await;

    assert!(!result.is_error, "{}", result.text);
    assert_eq!(
        access.get(task::TaskId::new(1)).unwrap().status(),
        task::TaskStatus::Deleted
    );
}

#[tokio::test]
async fn task_stop_rejects_zero_id_before_task_access() {
    let tool = TaskStopTool {
        access: seeded_access(),
    };

    let result = tool
        .call(serde_json::json!({"task_id": "0"}), &test_ctx())
        .await;

    assert!(result.is_error);
    assert!(result.text.contains("non-zero decimal number"));
}
