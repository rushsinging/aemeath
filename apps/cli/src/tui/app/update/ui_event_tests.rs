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

/// A3 Task 4: MessagesSync 退出 display —— 仅镜像 chat.messages，不产生 UserMessage 回显块，
/// 也不清除队列或占位（回显与占位清理由 UserMessagesAdded 负责）。
#[test]
fn test_update_ui_messages_sync_only_mirrors_no_echo() {
    let mut app = test_app();
    app.chat.messages.push(sdk::ChatMessage::user_text("first"));
    app.input.push_queue("a\nb\nc".to_string());
    let echo_id = sdk::InputId::new_v7();
    app.enqueue_submission_echo(echo_id.clone(), "[Copied Text 1]");
    let messages = vec![
        sdk::ChatMessage::user_text("first"),
        sdk::ChatMessage::user_text("a\nb\nc"),
    ];
    let (ui_tx, _ui_rx) = mpsc::channel(1);
    let spawn_refs = SpawnContextRefs { agent_client: None };

    app.update_ui(UiEvent::MessagesSync(messages), &ui_tx, &spawn_refs);

    // 镜像成功：chat.messages 更新到新的 msgs
    assert_eq!(app.chat.messages.len(), 2);

    // 不产生任何 UserMessage 回显块（退出 display）
    assert!(app.model.conversation.blocks.iter().all(|block| {
        !matches!(block, ConversationBlock::UserMessage { text, .. } if text == "a\nb\nc")
    }));

    // 队列与占位未被清除（归 UserMessagesAdded 负责）
    assert_eq!(app.input.queue_len(), 1, "MessagesSync 不应清队列");
    assert_eq!(
        app.model.conversation.queued_submissions.len(),
        1,
        "MessagesSync 不应清占位"
    );
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

/// Task 4: MessagesSync 退出 display，仅镜像 + 落盘
///
/// 场景：存在一条占位（id_a="hello"），收到包含 user_text("hello") 的 MessagesSync。
/// 期望：
/// - handler 后 self.chat.messages == msgs（镜像成功）
/// - 不产生任何 UserMessage 回显块（退出 display）
/// - 占位未被清除（清占位归 UserMessagesAdded 负责）
#[test]
fn test_messages_sync_no_display() {
    let mut app = test_app();
    let (ui_tx, _ui_rx) = mpsc::channel(1);
    let spawn_refs = make_spawn_refs();

    // 入队一条占位
    let id_a = sdk::InputId::new_v7();
    app.enqueue_submission_echo(id_a.clone(), "hello");
    assert_eq!(app.model.conversation.queued_submissions.len(), 1);

    // 构造包含该 user message 的 msgs
    let msgs = vec![sdk::ChatMessage::user_text("hello")];
    app.update_ui(UiEvent::MessagesSync(msgs.clone()), &ui_tx, &spawn_refs);

    // 镜像成功
    assert_eq!(
        app.chat.messages.len(),
        1,
        "MessagesSync 后 chat.messages 应镜像"
    );

    // 不产生 UserMessage 回显块
    let user_echo_count = app
        .model
        .conversation
        .blocks
        .iter()
        .filter(|b| matches!(b, ConversationBlock::UserMessage { .. }))
        .count();
    assert_eq!(
        user_echo_count, 0,
        "MessagesSync 不应产生 UserMessage 回显块（退出 display）"
    );

    // 占位未被清除（应由 UserMessagesAdded 负责）
    assert_eq!(
        app.model.conversation.queued_submissions.len(),
        1,
        "MessagesSync 不应清除占位（清占位归 UserMessagesAdded）"
    );
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
