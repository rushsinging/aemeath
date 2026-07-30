use super::*;

fn test_ctx() -> ToolExecutionContext {
    crate::domain::test_support::TestToolExecutionContextBuilder::new(std::path::PathBuf::from("."))
        .build()
}

fn setup() -> (
    Arc<task::TaskStore>,
    Arc<dyn task::TaskAccess>,
    task::TaskId,
) {
    let store = Arc::new(task::TaskStore::new());
    let access: Arc<dyn task::TaskAccess> = store.clone();
    access
        .create_batch(task::BatchCreateSpec::try_new("batch".into()).unwrap(), 1)
        .unwrap();
    let created = access
        .create_task(
            task::TaskCreateSpec::try_new(
                "任务".into(),
                "描述".into(),
                None,
                task::TaskPriority::Normal,
            )
            .unwrap(),
            2,
        )
        .unwrap();
    let id = created.value.id();
    (store, access, id)
}

#[tokio::test]
async fn task_update_uses_task_access_and_direct_complete_is_one_commit() {
    let (store, access, id) = setup();
    let revision_before = access.revision();
    let tool = TaskUpdateTool { access };

    let result = tool
        .call(
            serde_json::json!({"task_id": id.to_string(), "key": "status", "value": "completed"}),
            &test_ctx(),
        )
        .await;

    assert!(!result.is_error, "{}", result.text);
    assert_eq!(store.revision().get(), revision_before.get() + 1);
    let completed = store.get(id).unwrap();
    assert_eq!(completed.status(), task::TaskStatus::Completed);
    assert_eq!(completed.started_at(), completed.completed_at());
    assert!(result.text.contains("Status: Completed"));
    assert!(result.text.contains("automatically closed"));
    let data = result
        .data
        .expect("status update must return structured data");
    assert_eq!(data.status, "completed");
    let progress = data.progress.expect("status update must include progress");
    assert_eq!(progress.task_list.status, "archived");
    assert!(progress.lifecycle.auto_closed);
    assert_eq!(progress.recently_completed.len(), 1);
    assert!(progress.in_progress.is_empty());
    assert!(progress.ready.is_empty());
}

#[tokio::test]
async fn completing_the_last_task_auto_closes_the_list() {
    let (store, access, id) = setup();
    let tool = TaskUpdateTool { access };

    let completed = tool
        .call(
            serde_json::json!({"task_id": id.to_string(), "key": "status", "value": "completed"}),
            &test_ctx(),
        )
        .await;

    assert!(!completed.is_error, "{}", completed.text);
    assert!(completed.text.contains("automatically closed"));
    assert_eq!(store.current_batch(), None);
    assert!(
        completed
            .data
            .unwrap()
            .progress
            .unwrap()
            .lifecycle
            .auto_closed
    );
}

#[tokio::test]
async fn archived_task_list_can_be_reopened_by_explicit_list_id() {
    let (store, access, id) = setup();
    let tool = TaskUpdateTool {
        access: access.clone(),
    };
    tool.call(
        serde_json::json!({"task_id": id.to_string(), "key": "status", "value": "completed"}),
        &test_ctx(),
    )
    .await;

    let reopened = tool
        .call(
            serde_json::json!({"task_list_id": "1", "task_id": "1", "key": "status", "value": "pending"}),
            &test_ctx(),
        )
        .await;

    assert!(!reopened.is_error, "{}", reopened.text);
    assert!(reopened.text.contains("automatically reopened"));
    assert_eq!(store.current_batch(), Some(task::BatchId::new(1)));
    assert!(
        reopened
            .data
            .unwrap()
            .progress
            .unwrap()
            .lifecycle
            .auto_reopened
    );
}

#[tokio::test]
async fn archived_task_list_reopen_conflict_is_atomic() {
    let (store, access, id) = setup();
    let tool = TaskUpdateTool {
        access: access.clone(),
    };
    tool.call(
        serde_json::json!({"task_id": id.to_string(), "key": "status", "value": "completed"}),
        &test_ctx(),
    )
    .await;
    access
        .create_batch(task::BatchCreateSpec::try_new("other".into()).unwrap(), 3)
        .unwrap();
    let revision = access.revision();

    let conflict = tool
        .call(
            serde_json::json!({"task_list_id": "1", "task_id": "1", "key": "status", "value": "pending"}),
            &test_ctx(),
        )
        .await;

    assert!(conflict.is_error);
    assert!(conflict.text.contains("已经 active"));
    assert_eq!(access.revision(), revision);
    assert_eq!(store.get(id).unwrap().status(), task::TaskStatus::Completed);
}

#[tokio::test]
async fn task_update_rejects_legacy_owner_field() {
    let (_store, access, id) = setup();
    let tool = TaskUpdateTool { access };
    let result = tool
        .call(
            serde_json::json!({"task_id": id.to_string(), "key": "owner", "value": "alice"}),
            &test_ctx(),
        )
        .await;
    assert!(result.is_error);
    assert!(result.text.contains("unknown field"));
}

#[tokio::test]
async fn task_update_uses_typed_commands_for_mutable_fields() {
    let (store, access, id) = setup();
    let tool = TaskUpdateTool { access };

    for (key, value) in [
        ("subject", "新标题"),
        ("description", "新描述"),
        ("priority", "high"),
    ] {
        let result = tool
            .call(
                serde_json::json!({"task_id": id.to_string(), "key": key, "value": value}),
                &test_ctx(),
            )
            .await;
        assert!(!result.is_error, "{}", result.text);
    }
    let updated = store.get(id).unwrap();
    assert_eq!(updated.subject(), "新标题");
    assert_eq!(updated.description(), "新描述");
    assert_eq!(updated.priority(), task::TaskPriority::High);
}

#[tokio::test]
async fn task_update_rejects_blocked_by_field() {
    let (_store, access, id) = setup();
    let tool = TaskUpdateTool { access };

    let result = tool
        .call(
            serde_json::json!({"task_id": id.to_string(), "key": "blocked_by_id", "value": "2"}),
            &test_ctx(),
        )
        .await;

    assert!(result.is_error);
    assert!(result.text.contains("unknown field"));
    assert!(!result
        .text
        .contains("Valid keys: status, subject, description, priority, blocked_by_id"));
}

#[tokio::test]
async fn task_update_rejects_zero_ids_before_ohs() {
    let (_store, access, _id) = setup();
    let tool = TaskUpdateTool { access };

    let result = tool
        .call(
            serde_json::json!({"task_id": "0", "key": "status", "value": "completed"}),
            &test_ctx(),
        )
        .await;

    assert!(result.is_error);
    assert!(result.text.contains("non-zero decimal task ID"));
}

#[tokio::test]
async fn task_update_rejects_non_decimal_ids_before_ohs() {
    let (_store, access, _id) = setup();
    let tool = TaskUpdateTool { access };
    let result = tool
        .call(
            serde_json::json!({"task_id": "legacy-uuid", "key": "status", "value": "completed"}),
            &test_ctx(),
        )
        .await;
    assert!(result.is_error);
    assert!(result.text.contains("decimal task ID"));
}
