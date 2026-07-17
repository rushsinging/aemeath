use super::*;

fn test_ctx() -> ToolExecutionContext {
    ToolExecutionContext {
        workspace: project::wire_production_workspace(std::path::PathBuf::from(".")).expect("workspace 初始化成功").into_views(),
        run_id: "test-run".to_string(),
        cancel: tokio_util::sync::CancellationToken::new(),
        read_files: std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashSet::new())),
        resources: crate::api::ToolResources {
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
        agent_semaphore: std::sync::Arc::new(tokio::sync::Semaphore::new(4)),
        progress_tx: None,
        parent_session_id: None,
    }
}

async fn setup_task(store: &TaskStore) -> String {
    let task = store
        .create("原始标题".into(), "原始描述".into())
        .await;
    task.id
}

// --- key-value 模式基本测试 ---

#[tokio::test]
async fn test_update_status() {
    let store = Arc::new(TaskStore::new());
    let task_id = setup_task(&store).await;

    let tool = TaskUpdateTool {
        store: store.clone(),
    };
    let result = tool
        .call(
            serde_json::json!({"taskId": task_id, "key": "status", "value": "in_progress"}),
            &test_ctx(),
        )
        .await;

    assert!(!result.is_error);
    let task = store.get(&task_id).await.unwrap();
    assert_eq!(task.status, TaskStatus::InProgress);
}

#[tokio::test]
async fn test_update_subject() {
    let store = Arc::new(TaskStore::new());
    let task_id = setup_task(&store).await;

    let tool = TaskUpdateTool {
        store: store.clone(),
    };
    tool.call(
        serde_json::json!({"taskId": task_id, "key": "subject", "value": "新标题"}),
        &test_ctx(),
    )
    .await;

    let task = store.get(&task_id).await.unwrap();
    assert_eq!(task.subject, "新标题");
}

#[tokio::test]
async fn test_update_description() {
    let store = Arc::new(TaskStore::new());
    let task_id = setup_task(&store).await;

    let tool = TaskUpdateTool {
        store: store.clone(),
    };
    tool.call(
        serde_json::json!({"taskId": task_id, "key": "description", "value": "新描述"}),
        &test_ctx(),
    )
    .await;

    let task = store.get(&task_id).await.unwrap();
    assert_eq!(task.description, "新描述");
}

#[tokio::test]
async fn test_update_owner() {
    let store = Arc::new(TaskStore::new());
    let task_id = setup_task(&store).await;

    let tool = TaskUpdateTool {
        store: store.clone(),
    };
    tool.call(
        serde_json::json!({"taskId": task_id, "key": "owner", "value": "alice"}),
        &test_ctx(),
    )
    .await;

    let task = store.get(&task_id).await.unwrap();
    assert_eq!(task.owner.as_deref(), Some("alice"));
}

#[tokio::test]
async fn test_update_priority() {
    let store = Arc::new(TaskStore::new());
    let task_id = setup_task(&store).await;

    let tool = TaskUpdateTool {
        store: store.clone(),
    };
    tool.call(
        serde_json::json!({"taskId": task_id, "key": "priority", "value": "high"}),
        &test_ctx(),
    )
    .await;

    let task = store.get(&task_id).await.unwrap();
    assert_eq!(task.priority, TaskPriority::High);
}

#[tokio::test]
async fn test_invalid_key_returns_error() {
    let store = Arc::new(TaskStore::new());
    let task_id = setup_task(&store).await;

    let tool = TaskUpdateTool {
        store: store.clone(),
    };
    let result = tool
        .call(
            serde_json::json!({"taskId": task_id, "key": "unknown_field", "value": "x"}),
            &test_ctx(),
        )
        .await;

    assert!(result.is_error);
    assert!(result.text.contains("unknown field"));
}

#[tokio::test]
async fn test_non_string_value_returns_error() {
    let store = Arc::new(TaskStore::new());
    let task_id = setup_task(&store).await;

    let tool = TaskUpdateTool {
        store: store.clone(),
    };
    let result = tool
        .call(
            serde_json::json!({"taskId": task_id, "key": "status", "value": 123}),
            &test_ctx(),
        )
        .await;

    assert!(result.is_error);
    assert!(result.text.contains("must be a string"));
}

#[tokio::test]
async fn test_completed_status_output_contains_status_text() {
    // hook 检测 output 中的 "Status: Completed" 来触发 TaskCompleted 事件
    let store = Arc::new(TaskStore::new());
    let task_id = setup_task(&store).await;

    let tool = TaskUpdateTool {
        store: store.clone(),
    };
    let result = tool
        .call(
            serde_json::json!({"taskId": task_id, "key": "status", "value": "completed"}),
            &test_ctx(),
        )
        .await;

    assert!(!result.is_error);
    assert!(
        result.text.contains("Status: Completed"),
        "output 应包含 'Status: Completed' 以触发 hook: {}",
        result.text
    );
}

#[tokio::test]
async fn test_blocked_by_id_adds_dependency() {
    let store = Arc::new(TaskStore::new());
    let blocking_task = store
        .create("阻塞任务".into(), "描述".into())
        .await;
    let blocked_task = setup_task(&store).await;

    let tool = TaskUpdateTool {
        store: store.clone(),
    };
    let result = tool
        .call(
            serde_json::json!({"taskId": blocked_task, "key": "blocked_by_id", "value": blocking_task.id}),
            &test_ctx(),
        )
        .await;

    assert!(!result.is_error);
    let task = store.get(&blocked_task).await.unwrap();
    assert!(task.blocked_by.contains(&blocking_task.id));
}

#[tokio::test]
async fn test_blocked_by_id_nonexistent_returns_error() {
    let store = Arc::new(TaskStore::new());
    let task_id = setup_task(&store).await;

    let tool = TaskUpdateTool {
        store: store.clone(),
    };
    let result = tool
        .call(
            serde_json::json!({"taskId": task_id, "key": "blocked_by_id", "value": "nonexistent"}),
            &test_ctx(),
        )
        .await;

    assert!(result.is_error);
    assert!(result.text.contains("not found"));
}
