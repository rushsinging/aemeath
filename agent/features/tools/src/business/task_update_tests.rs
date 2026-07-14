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

async fn setup_task(store: &TaskStore) -> String {
    let task = store
        .create("原始标题".into(), "原始描述".into(), Some("处理中".into()))
        .await;
    task.id
}

#[tokio::test]
async fn test_empty_subject_does_not_clobber() {
    let store = Arc::new(TaskStore::new());
    let task_id = setup_task(&store).await;

    let tool = TaskUpdateTool {
        store: store.clone(),
    };
    let result = tool
        .call(
            serde_json::json!({"taskId": task_id, "subject": "", "addBlockedBy": ["999"]}),
            &test_ctx(),
        )
        .await;

    // add_blocked_by 引用不存在的 id 仍不影响 subject 守卫逻辑
    assert!(!result.is_error);
    let task = store.get(&task_id).await.unwrap();
    assert_eq!(task.subject, "原始标题");
}

#[tokio::test]
async fn test_empty_description_does_not_clobber() {
    let store = Arc::new(TaskStore::new());
    let task_id = setup_task(&store).await;

    let tool = TaskUpdateTool {
        store: store.clone(),
    };
    tool.call(
        serde_json::json!({"taskId": task_id, "description": ""}),
        &test_ctx(),
    )
    .await;

    let task = store.get(&task_id).await.unwrap();
    assert_eq!(task.description, "原始描述");
}

#[tokio::test]
async fn test_empty_active_form_does_not_clobber() {
    let store = Arc::new(TaskStore::new());
    let task_id = setup_task(&store).await;

    let tool = TaskUpdateTool {
        store: store.clone(),
    };
    tool.call(
        serde_json::json!({"taskId": task_id, "activeForm": ""}),
        &test_ctx(),
    )
    .await;

    let task = store.get(&task_id).await.unwrap();
    assert_eq!(task.active_form.as_deref(), Some("处理中"));
}

#[tokio::test]
async fn test_empty_owner_does_not_clobber() {
    let store = Arc::new(TaskStore::new());
    let task_id = setup_task(&store).await;

    // 先设置 owner
    store
        .update(&task_id, |t| t.owner = Some("alice".into()))
        .await;

    let tool = TaskUpdateTool {
        store: store.clone(),
    };
    tool.call(
        serde_json::json!({"taskId": task_id, "owner": ""}),
        &test_ctx(),
    )
    .await;

    let task = store.get(&task_id).await.unwrap();
    assert_eq!(task.owner.as_deref(), Some("alice"));
}

#[tokio::test]
async fn test_empty_progress_message_does_not_clobber() {
    let store = Arc::new(TaskStore::new());
    let task_id = setup_task(&store).await;

    // 先设置 progress_message
    store
        .update(&task_id, |t| {
            t.progress_message = Some("50% 完成".into());
        })
        .await;

    let tool = TaskUpdateTool {
        store: store.clone(),
    };
    tool.call(
        serde_json::json!({"taskId": task_id, "progressMessage": ""}),
        &test_ctx(),
    )
    .await;

    let task = store.get(&task_id).await.unwrap();
    assert_eq!(task.progress_message.as_deref(), Some("50% 完成"));
}

#[tokio::test]
async fn test_non_empty_subject_still_updates() {
    let store = Arc::new(TaskStore::new());
    let task_id = setup_task(&store).await;

    let tool = TaskUpdateTool {
        store: store.clone(),
    };
    tool.call(
        serde_json::json!({"taskId": task_id, "subject": "新标题"}),
        &test_ctx(),
    )
    .await;

    let task = store.get(&task_id).await.unwrap();
    assert_eq!(task.subject, "新标题");
}

// --- #979: 空白占位符拦截，标点占位符放行（靠 result 回填 + prompt 从源头减少）---

#[tokio::test]
async fn test_whitespace_subject_does_not_clobber() {
    let store = Arc::new(TaskStore::new());
    let task_id = setup_task(&store).await;

    let tool = TaskUpdateTool {
        store: store.clone(),
    };
    tool.call(
        serde_json::json!({"taskId": task_id, "subject": "  "}),
        &test_ctx(),
    )
    .await;

    let task = store.get(&task_id).await.unwrap();
    assert_eq!(task.subject, "原始标题");
}

#[tokio::test]
async fn test_whitespace_description_does_not_clobber() {
    let store = Arc::new(TaskStore::new());
    let task_id = setup_task(&store).await;

    let tool = TaskUpdateTool {
        store: store.clone(),
    };
    tool.call(
        serde_json::json!({"taskId": task_id, "description": "   "}),
        &test_ctx(),
    )
    .await;

    let task = store.get(&task_id).await.unwrap();
    assert_eq!(task.description, "原始描述");
}

#[tokio::test]
async fn test_whitespace_active_form_does_not_clobber() {
    let store = Arc::new(TaskStore::new());
    let task_id = setup_task(&store).await;

    let tool = TaskUpdateTool {
        store: store.clone(),
    };
    tool.call(
        serde_json::json!({"taskId": task_id, "activeForm": "\t"}),
        &test_ctx(),
    )
    .await;

    let task = store.get(&task_id).await.unwrap();
    assert_eq!(task.active_form.as_deref(), Some("处理中"));
}

#[tokio::test]
async fn test_whitespace_owner_does_not_clobber() {
    let store = Arc::new(TaskStore::new());
    let task_id = setup_task(&store).await;

    store
        .update(&task_id, |t| t.owner = Some("alice".into()))
        .await;

    let tool = TaskUpdateTool {
        store: store.clone(),
    };
    tool.call(
        serde_json::json!({"taskId": task_id, "owner": " "}),
        &test_ctx(),
    )
    .await;

    let task = store.get(&task_id).await.unwrap();
    assert_eq!(task.owner.as_deref(), Some("alice"));
}

#[tokio::test]
async fn test_whitespace_progress_message_does_not_clobber() {
    let store = Arc::new(TaskStore::new());
    let task_id = setup_task(&store).await;

    store
        .update(&task_id, |t| {
            t.progress_message = Some("50% 完成".into());
        })
        .await;

    let tool = TaskUpdateTool {
        store: store.clone(),
    };
    tool.call(
        serde_json::json!({"taskId": task_id, "progressMessage": "  "}),
        &test_ctx(),
    )
    .await;

    let task = store.get(&task_id).await.unwrap();
    assert_eq!(task.progress_message.as_deref(), Some("50% 完成"));
}

// --- is_placeholder 单元测试 ---

#[test]
fn test_is_placeholder_empty() {
    assert!(is_placeholder(""));
}

#[test]
fn test_is_placeholder_whitespace() {
    assert!(is_placeholder("  "));
    assert!(is_placeholder("\t\n"));
}

#[test]
fn test_is_placeholder_not_punctuation() {
    // 标点占位符不拦截——靠 result 回填 + prompt 从源头减少
    assert!(!is_placeholder(","));
    assert!(!is_placeholder("-"));
}

#[test]
fn test_is_placeholder_valid_values() {
    assert!(!is_placeholder("原始标题"));
    assert!(!is_placeholder("Fix bug"));
    assert!(!is_placeholder("A-1"));
    assert!(!is_placeholder("50% 完成"));
}
