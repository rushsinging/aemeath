use super::*;

#[test]
fn test_create_workspace_stores_workspace() {
    let state = AppState::default();

    let workspace = state
        .create_workspace(
            "tenant-a".to_string(),
            "Main".to_string(),
            "anthropic".to_string(),
            "claude".to_string(),
        )
        .expect("workspace created");

    assert_eq!(workspace.name, "Main");
    assert_eq!(state.get_workspace(&workspace.id), Ok(workspace));
}

#[test]
fn test_create_workspace_rejects_empty_name() {
    let state = AppState::default();

    let result = state.create_workspace(
        "tenant-a".to_string(),
        " ".to_string(),
        "anthropic".to_string(),
        "claude".to_string(),
    );

    assert!(matches!(
        result,
        Err(StoreError::InvalidInput { field }) if field == "name"
    ));
}

#[test]
fn test_add_message_publishes_board_update() {
    let state = AppState::default();
    let workspace = state
        .create_workspace("t1".into(), "Main".into(), "p".into(), "m".into())
        .expect("workspace created");
    let chat = state
        .create_chat(&workspace.id, "General".into())
        .expect("chat created");
    let mut updates = state.subscribe_board_updates();

    let result = state
        .add_message(
            &workspace.id,
            &chat.id,
            "user".into(),
            "hello".into(),
            "k1".into(),
        )
        .expect("message added");

    let update = updates.try_recv().expect("update published");
    assert_eq!(update.workspace_id, workspace.id);
    assert_eq!(update.event_kind, BoardEventKind::MessageAdded);
    assert_eq!(update.message.id, result.message.id);
}

#[test]
fn test_add_message_deduplicated_request_does_not_publish_board_update() {
    let state = AppState::default();
    let workspace = state
        .create_workspace("t1".into(), "Main".into(), "p".into(), "m".into())
        .expect("workspace created");
    let chat = state
        .create_chat(&workspace.id, "General".into())
        .expect("chat created");

    state
        .add_message(
            &workspace.id,
            &chat.id,
            "user".into(),
            "hello".into(),
            "k1".into(),
        )
        .expect("message added");
    let mut updates = state.subscribe_board_updates();

    let result = state
        .add_message(
            &workspace.id,
            &chat.id,
            "user".into(),
            "hello again".into(),
            "k1".into(),
        )
        .expect("message deduplicated");

    assert!(result.deduplicated);
    assert!(updates.try_recv().is_err());
}

#[test]
fn test_add_message_deduplicates_by_idempotency_key() {
    let state = AppState::default();
    let workspace = state
        .create_workspace(
            "tenant-a".to_string(),
            "Main".to_string(),
            "anthropic".to_string(),
            "claude".to_string(),
        )
        .expect("workspace created");
    let chat = state
        .create_chat(&workspace.id, "General".to_string())
        .expect("chat created");

    let first = state
        .add_message(
            &workspace.id,
            &chat.id,
            "user".to_string(),
            "hello".to_string(),
            "same-key".to_string(),
        )
        .expect("message added");
    let second = state
        .add_message(
            &workspace.id,
            &chat.id,
            "user".to_string(),
            "changed".to_string(),
            "same-key".to_string(),
        )
        .expect("message deduplicated");

    assert!(!first.deduplicated);
    assert!(second.deduplicated);
    assert_eq!(first.message.id, second.message.id);
    assert_eq!(second.message.content, "hello");
}

#[test]
fn test_update_chat_changes_title_and_version() {
    let state = AppState::default();
    let workspace = state
        .create_workspace("t1".into(), "Main".into(), "p".into(), "m".into())
        .expect("workspace created");
    let chat = state
        .create_chat(&workspace.id, "Old".into())
        .expect("chat created");

    let updated = state
        .update_chat(&workspace.id, &chat.id, Some("New".into()), None)
        .expect("chat updated");

    assert_eq!(updated.title, "New");
    assert_eq!(updated.version, 2);
}

#[test]
fn test_list_chat_messages_returns_newest_page_with_cursor() {
    let state = AppState::default();
    let workspace = state
        .create_workspace("t1".into(), "Main".into(), "p".into(), "m".into())
        .expect("workspace created");
    let chat = state
        .create_chat(&workspace.id, "General".into())
        .expect("chat created");
    let first = add_test_message(&state, &workspace.id, &chat.id, "first", "k1");
    let second = add_test_message(&state, &workspace.id, &chat.id, "second", "k2");
    let third = add_test_message(&state, &workspace.id, &chat.id, "third", "k3");

    let page = state
        .list_chat_messages(&workspace.id, &chat.id, 2, None)
        .expect("messages listed");

    assert_eq!(page.messages.len(), 2);
    assert_eq!(page.messages[0].id, third.id);
    assert_eq!(page.messages[1].id, second.id);
    assert!(page.has_more);
    assert_eq!(page.next_cursor, Some(second.id));
    assert_ne!(page.next_cursor, Some(first.id));
}

#[test]
fn test_list_chat_messages_uses_before_cursor_for_older_page() {
    let state = AppState::default();
    let workspace = state
        .create_workspace("t1".into(), "Main".into(), "p".into(), "m".into())
        .expect("workspace created");
    let chat = state
        .create_chat(&workspace.id, "General".into())
        .expect("chat created");
    let first = add_test_message(&state, &workspace.id, &chat.id, "first", "k1");
    let second = add_test_message(&state, &workspace.id, &chat.id, "second", "k2");
    let third = add_test_message(&state, &workspace.id, &chat.id, "third", "k3");

    let page = state
        .list_chat_messages(&workspace.id, &chat.id, 2, Some(&third.id))
        .expect("messages listed");

    assert_eq!(page.messages.len(), 2);
    assert_eq!(page.messages[0].id, second.id);
    assert_eq!(page.messages[1].id, first.id);
    assert!(!page.has_more);
    assert_eq!(page.next_cursor, None);
}

#[test]
fn test_list_chat_messages_rejects_unknown_chat() {
    let state = AppState::default();
    let workspace = state
        .create_workspace("t1".into(), "Main".into(), "p".into(), "m".into())
        .expect("workspace created");

    let result = state.list_chat_messages(&workspace.id, "missing", 50, None);

    assert!(matches!(result, Err(StoreError::NotFound { entity }) if entity == "chat"));
}

#[test]
fn test_analyze_message_classifies_requirement() {
    let analysis = analyze_message_type("请实现一个新功能");

    assert_eq!(analysis.message_type, "requirement");
}

#[test]
fn test_analyze_message_classifies_feedback() {
    let analysis = analyze_message_type("这里有个 bug 需要修复");

    assert_eq!(analysis.message_type, "feedback");
}

fn add_test_message(
    state: &AppState,
    workspace_id: &str,
    chat_id: &str,
    content: &str,
    idempotency_key: &str,
) -> ChatMessage {
    state
        .add_message(
            workspace_id,
            chat_id,
            "user".into(),
            content.into(),
            idempotency_key.into(),
        )
        .expect("message added")
        .message
}
