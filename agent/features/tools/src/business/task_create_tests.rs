use super::*;

fn test_ctx() -> ToolExecutionContext {
    ToolExecutionContext {
        workspace: project::api::WorkspaceService::new(std::path::PathBuf::from(".")),
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

#[tokio::test]
async fn test_empty_owner_not_stored() {
    let store = Arc::new(TaskStore::new());
    let tool = TaskCreateTool {
        store: store.clone(),
    };

    tool.call(
        serde_json::json!({
            "subject": "测试任务",
            "description": "描述",
            "owner": ""
        }),
        &test_ctx(),
    )
    .await;

    let snap = store.snapshot().await;
    let task = &snap.tasks[0];
    assert_eq!(task.owner, None);
}

#[tokio::test]
async fn test_empty_session_id_not_stored() {
    let store = Arc::new(TaskStore::new());
    let tool = TaskCreateTool {
        store: store.clone(),
    };

    tool.call(
        serde_json::json!({
            "subject": "测试任务",
            "description": "描述",
            "sessionId": ""
        }),
        &test_ctx(),
    )
    .await;

    let snap = store.snapshot().await;
    let task = &snap.tasks[0];
    assert_eq!(task.session_id, None);
}

#[tokio::test]
async fn test_non_empty_owner_still_stored() {
    let store = Arc::new(TaskStore::new());
    let tool = TaskCreateTool {
        store: store.clone(),
    };

    tool.call(
        serde_json::json!({
            "subject": "测试任务",
            "description": "描述",
            "owner": "alice"
        }),
        &test_ctx(),
    )
    .await;

    let snap = store.snapshot().await;
    let task = &snap.tasks[0];
    assert_eq!(task.owner.as_deref(), Some("alice"));
}

// --- #979: 空白占位符拦截 ---

#[tokio::test]
async fn test_whitespace_owner_not_stored() {
    let store = Arc::new(TaskStore::new());
    let tool = TaskCreateTool {
        store: store.clone(),
    };

    tool.call(
        serde_json::json!({
            "subject": "测试任务",
            "description": "描述",
            "owner": "  "
        }),
        &test_ctx(),
    )
    .await;

    let snap = store.snapshot().await;
    let task = &snap.tasks[0];
    assert_eq!(task.owner, None);
}

#[tokio::test]
async fn test_whitespace_session_id_not_stored() {
    let store = Arc::new(TaskStore::new());
    let tool = TaskCreateTool {
        store: store.clone(),
    };

    tool.call(
        serde_json::json!({
            "subject": "测试任务",
            "description": "描述",
            "sessionId": "\t"
        }),
        &test_ctx(),
    )
    .await;

    let snap = store.snapshot().await;
    let task = &snap.tasks[0];
    assert_eq!(task.session_id, None);
}
