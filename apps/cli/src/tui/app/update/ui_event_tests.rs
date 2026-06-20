use super::*;
use crate::tui::effect::session::processing::SpawnContextRefs;
use crate::tui::model::conversation::block::ConversationBlock;
use std::path::PathBuf;

fn make_spawn_refs() -> SpawnContextRefs {
    SpawnContextRefs { agent_client: None }
}

fn test_app() -> App {
    App::new(
        "test-session".to_string(),
        PathBuf::from("/tmp"),
        "test-model".to_string(),
    )
}

#[test]
fn test_update_ui_drain_queued_input_clears_placeholder_without_user_echo() {
    let mut app = test_app();
    app.input.push_queue("a\nb\nc".to_string());
    app.enqueue_submission_echo(sdk::InputId::new_v7(), "[Copied Text 1]");
    let (reply_tx, mut reply_rx) = tokio::sync::oneshot::channel();
    let (ui_tx, _ui_rx) = mpsc::channel(1);
    let spawn_refs = SpawnContextRefs { agent_client: None };

    app.update_ui(UiEvent::DrainQueuedInput { reply_tx }, &ui_tx, &spawn_refs);

    assert_eq!(reply_rx.try_recv(), Ok(vec!["a\nb\nc".to_string()]));
    assert!(app.model.conversation.blocks.iter().all(
        |block| !matches!(block, ConversationBlock::UserMessage { text, .. } if text == "a\nb\nc")
    ));
    assert!(app
        .model
        .conversation
        .blocks
        .iter()
        .all(|block| !matches!(block, ConversationBlock::QueuedUserMessage { .. })));
}

#[test]
fn test_update_ui_messages_sync_echoes_original_user_message() {
    let mut app = test_app();
    app.chat.messages.push(sdk::ChatMessage::user_text("first"));
    app.input.push_queue("a\nb\nc".to_string());
    app.enqueue_submission_echo(sdk::InputId::new_v7(), "[Copied Text 1]");
    let messages = vec![
        sdk::ChatMessage::user_text("first"),
        sdk::ChatMessage::user_text("a\nb\nc"),
    ];
    let (ui_tx, _ui_rx) = mpsc::channel(1);
    let spawn_refs = SpawnContextRefs { agent_client: None };

    app.update_ui(UiEvent::MessagesSync(messages), &ui_tx, &spawn_refs);

    assert_eq!(app.input.queue_len(), 0);
    assert!(app.model.conversation.blocks.iter().any(|block| {
        matches!(block, ConversationBlock::UserMessage { text, .. } if text == "a\nb\nc")
    }));
    assert!(app
        .model
        .conversation
        .blocks
        .iter()
        .all(|block| !matches!(block, ConversationBlock::QueuedUserMessage { .. })));
}

#[test]
fn test_update_ui_messages_sync_does_not_echo_system_generated_user_message() {
    let mut app = test_app();
    app.chat.messages.push(sdk::ChatMessage::user_text("first"));
    let reminder = "<system-reminder>\nStop hook blocked stopping.\n</system-reminder>";
    let messages = vec![
        sdk::ChatMessage::user_text("first"),
        sdk::ChatMessage::system_generated_user_text(reminder),
    ];
    let (ui_tx, _ui_rx) = mpsc::channel(1);
    let spawn_refs = SpawnContextRefs { agent_client: None };

    app.update_ui(UiEvent::MessagesSync(messages), &ui_tx, &spawn_refs);

    assert!(app.model.conversation.blocks.iter().all(|block| {
        !matches!(block, ConversationBlock::UserMessage { text, .. } if text == reminder)
    }));
}

/// Task 3: UserMessagesAdded 按 id 清占位 + 顺序回显
///
/// 场景：入队两条占位（A="hi"，B="yo"）；
/// handler 收到 UserMessagesAdded([{id:A,"hi"},{id:B,"yo"}])
/// → A/B 占位全清、按序追加两条正式 UserMessage 回显 "hi"/"yo"，无残留占位。
#[test]
fn test_user_messages_added_consumes_placeholders_and_echoes_in_order() {
    let mut app = test_app();
    let (ui_tx, _ui_rx) = mpsc::channel(1);
    let spawn_refs = make_spawn_refs();

    // 入队两条占位（id_a / id_b）
    let id_a = sdk::InputId::new_v7();
    let id_b = sdk::InputId::new_v7();
    app.enqueue_submission_echo(id_a.clone(), "hi");
    app.enqueue_submission_echo(id_b.clone(), "yo");

    // 确认两条占位已在 model 中
    assert_eq!(app.model.conversation.queued_submissions.len(), 2);

    // 触发 handler
    let items = vec![
        sdk::AddedInput {
            id: id_a.clone(),
            text: "hi".to_string(),
        },
        sdk::AddedInput {
            id: id_b.clone(),
            text: "yo".to_string(),
        },
    ];
    app.update_ui(UiEvent::UserMessagesAdded(items), &ui_tx, &spawn_refs);

    // 占位全清
    assert!(
        app.model.conversation.queued_submissions.is_empty(),
        "handler 执行后不应有残留占位"
    );
    let queued_blocks = app
        .model
        .conversation
        .blocks
        .iter()
        .filter(|b| matches!(b, ConversationBlock::QueuedUserMessage { .. }))
        .count();
    assert_eq!(queued_blocks, 0, "不应有残留 QueuedUserMessage 块");

    // 按序追加两条正式 UserMessage
    let user_echo_texts: Vec<&str> = app
        .model
        .conversation
        .blocks
        .iter()
        .filter_map(|b| {
            if let ConversationBlock::UserMessage { text, .. } = b {
                Some(text.as_str())
            } else {
                None
            }
        })
        .collect();
    assert_eq!(
        user_echo_texts,
        vec!["hi", "yo"],
        "应按序追加两条正式 UserMessage 回显"
    );
}
