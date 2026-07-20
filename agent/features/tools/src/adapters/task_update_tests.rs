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
async fn task_update_uses_typed_commands_for_fields_and_dependency() {
    let (store, access, id) = setup();
    let dependency = access
        .create_task(
            task::TaskCreateSpec::try_new(
                "前置".into(),
                String::new(),
                None,
                task::TaskPriority::Normal,
            )
            .unwrap(),
            3,
        )
        .unwrap()
        .value
        .id();
    let tool = TaskUpdateTool { access };

    for (key, value) in [
        ("subject", "新标题"),
        ("description", "新描述"),
        ("priority", "high"),
        ("blocked_by_id", &dependency.to_string()),
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
    assert_eq!(updated.blocked_by(), &[dependency]);
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
