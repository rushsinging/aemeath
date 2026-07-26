use super::*;

fn test_ctx() -> ToolExecutionContext {
    crate::domain::test_support::TestToolExecutionContextBuilder::new(std::path::PathBuf::from("."))
        .build()
}

#[tokio::test]
async fn task_list_uses_current_batch_sequences_for_ids_and_dependencies() {
    let access: Arc<dyn task::TaskAccess> = Arc::new(task::TaskStore::new());
    access
        .create_batch(task::BatchCreateSpec::try_new("旧请求".into()).unwrap(), 1)
        .unwrap();
    access
        .create_task(
            task::TaskCreateSpec::try_new(
                "旧任务".into(),
                String::new(),
                None,
                task::TaskPriority::Normal,
            )
            .unwrap(),
            2,
        )
        .unwrap();
    access
        .create_batch(
            task::BatchCreateSpec::try_new("当前请求".into()).unwrap(),
            3,
        )
        .unwrap();
    let first = access
        .create_task(
            task::TaskCreateSpec::try_new(
                "前置".into(),
                String::new(),
                None,
                task::TaskPriority::Normal,
            )
            .unwrap(),
            4,
        )
        .unwrap()
        .value;
    let second = access
        .create_task(
            task::TaskCreateSpec::try_new(
                "后续".into(),
                String::new(),
                None,
                task::TaskPriority::Normal,
            )
            .unwrap(),
            5,
        )
        .unwrap()
        .value;
    access.add_dependency(second.id(), first.id(), 6).unwrap();

    let result = TaskListTool { access }
        .call(serde_json::json!({}), &test_ctx())
        .await;

    assert!(!result.is_error, "{}", result.text);
    let data = serde_json::to_value(result.data.unwrap()).unwrap();
    let tasks = data["tasks"].clone();
    assert_eq!(tasks.as_array().unwrap().len(), 2);
    assert_eq!(tasks[0]["id"], "1");
    assert_eq!(tasks[1]["id"], "2");
    assert_eq!(tasks[1]["blocked_by"], serde_json::json!(["1"]));
    assert_eq!(data["task_list"]["id"], "2");
    assert_eq!(data["stats"]["total"], 2);
    assert!(result.text.starts_with("2 tasks"));
}

#[tokio::test]
async fn task_list_queries_archived_batch_by_id() {
    let access: Arc<dyn task::TaskAccess> = Arc::new(task::TaskStore::new());
    access
        .create_batch(
            task::BatchCreateSpec::try_new("历史请求".into()).unwrap(),
            1,
        )
        .unwrap();
    access
        .create_task(
            task::TaskCreateSpec::try_new(
                "历史任务".into(),
                String::new(),
                None,
                task::TaskPriority::Normal,
            )
            .unwrap(),
            2,
        )
        .unwrap();
    access.archive_batch(task::BatchId::new(1)).unwrap();

    let result = TaskListTool { access }
        .call(serde_json::json!({"task_list_id": "1"}), &test_ctx())
        .await;

    assert!(!result.is_error, "{}", result.text);
    let data = serde_json::to_value(result.data.unwrap()).unwrap();
    assert_eq!(data["task_list"]["status"], "archived");
    assert_eq!(data["tasks"][0]["subject"], "历史任务");
}
