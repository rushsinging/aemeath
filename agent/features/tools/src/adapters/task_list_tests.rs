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
                "高优先级".into(),
                String::new(),
                None,
                task::TaskPriority::High,
            )
            .unwrap(),
            2,
        )
        .unwrap();
    access
        .create_task(
            task::TaskCreateSpec::try_new(
                "普通优先级".into(),
                String::new(),
                None,
                task::TaskPriority::Normal,
            )
            .unwrap(),
            3,
        )
        .unwrap();
    access
}

#[tokio::test]
async fn task_list_filters_live_tasks_by_priority() {
    let tool = TaskListTool {
        access: seeded_access(),
    };

    let result = tool
        .call(serde_json::json!({"priority": "high"}), &test_ctx())
        .await;

    assert!(!result.is_error, "{}", result.text);
    let value = serde_json::to_value(result.data.unwrap()).expect("serialize task list result");
    assert_eq!(value["tasks"].as_array().unwrap().len(), 1);
    assert_eq!(value["tasks"][0]["subject"], "高优先级");
}
