use super::*;
use crate::tui::effect::session::processing::SpawnContextRefs;
use crate::tui::model::conversation::block::ConversationBlock;
use std::path::PathBuf;

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
