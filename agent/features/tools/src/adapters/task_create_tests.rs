use super::*;

fn test_ctx() -> ToolExecutionContext {
    ToolExecutionContext {
        workspace: project::wire_production_workspace(std::path::PathBuf::from("."))
            .expect("workspace 初始化成功")
            .into_views(),
        run_id: "test-run".to_string(),
        cancel: tokio_util::sync::CancellationToken::new(),
        read_files: Arc::new(std::sync::Mutex::new(std::collections::HashSet::new())),
        resources: crate::domain::ToolResources {
            agent_runner: None,
            registry: None,
            memory_config: share::config::MemoryConfig::default(),
            lang: "en".to_string(),
            allow_all: false,
        },
        session_reminders: None,
        plan_mode: None,
        max_tool_concurrency: 4,
        max_agent_concurrency: 4,
        agent_semaphore: Arc::new(tokio::sync::Semaphore::new(4)),
        progress_tx: None,
        parent_session_id: None,
    }
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
