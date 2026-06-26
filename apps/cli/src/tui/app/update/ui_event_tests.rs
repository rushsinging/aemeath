use super::*;
use crate::tui::effect::session::processing::SpawnContextRefs;
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

/// A3 Task 4: MessagesSync 退出 display —— 仅镜像 chat.messages，不产生 UserMessage 回显块，
/// 也不清除占位（回显与占位清理由 UserMessagesAdded 负责）。
#[test]
fn test_update_ui_messages_sync_only_mirrors_no_echo() {
    let mut app = test_app();
    app.chat.messages.push(sdk::ChatMessage::user_text("first"));
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
    assert!(app.model.conversation.timeline.items().iter().all(|item| {
        !matches!(item, crate::tui::model::output_timeline::OutputTimelineItem::UserMessage { text, .. } if text == "a\nb\nc")
    }));

    // 占位未被清除（归 UserMessagesAdded 负责）
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

    assert!(app.model.conversation.timeline.items().iter().all(|item| {
        !matches!(item, crate::tui::model::output_timeline::OutputTimelineItem::UserMessage { text, .. } if text == reminder)
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
        .timeline
        .items()
        .iter()
        .filter(|b| {
            matches!(
                b,
                crate::tui::model::output_timeline::OutputTimelineItem::UserMessage { .. }
            )
        })
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
              sdk::ChatMessage {
                  role: "user".to_string(),
                  content: vec![sdk::ContentBlock::text("hi")],
                  metadata: None,
                  input_id: Some(id_a.clone()),
              },
              sdk::ChatMessage {
                  role: "user".to_string(),
                  content: vec![sdk::ContentBlock::text("yo")],
                  metadata: None,
                  input_id: Some(id_b.clone()),
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
        .timeline
        .items()
        .iter()
        .filter(|b| {
            matches!(
                b,
                crate::tui::model::output_timeline::OutputTimelineItem::QueuedUserMessage { .. }
            )
        })
        .count();
    assert_eq!(queued_blocks, 0, "不应有残留 QueuedUserMessage 块");

    // 按序追加两条正式 UserMessage
    let user_echo_texts: Vec<&str> = app
        .model
        .conversation
        .timeline
        .items()
        .iter()
        .filter_map(|b| {
            if let crate::tui::model::output_timeline::OutputTimelineItem::UserMessage {
                text,
                ..
            } = b
            {
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

/// #507 回归：UserMessagesAdded 携带 ChatMessage（typed blocks 含 Image.placeholder）
/// 时，回显文本应经 message.text_content() 还原出用户视角完整文本（含占位符）。
///
/// 场景：用户输入"看图[Image #1]"（TUI 端 enqueue_submission_echo 用 display_text
/// 写入排队块）；runtime 端构造 ChatMessage（content 含 Image { placeholder } + 对应
/// input_id），通过 UserMessagesAdded 携带。
/// handler 收到后：
/// - 按 message.input_id 清除对应占位块
/// - 用 message.text_content() 还原 "看图[Image #1]"，写入 UserMessage 回显
#[test]
fn test_user_messages_added_echoes_image_placeholder_from_message() {
    use sdk::{ChatInputImage, ChatMessage};
    let mut app = test_app();
    let (ui_tx, _ui_rx) = mpsc::channel(1);
    let spawn_refs = make_spawn_refs();

    // 用户提交"看图[Image #1]"——TUI 端 enqueue 占位（display_text 含占位符）
    let input_id = sdk::InputId::new_v7();
    app.enqueue_submission_echo(input_id.clone(), "看图[Image #1]");
    assert_eq!(app.model.conversation.queued_submissions.len(), 1);

    // runtime 端构造的 ChatMessage：image block 携带 placeholder（用于 text_content 还原位置）
    let mut message = ChatMessage::user_with_images(
        "看图[Image #1]",
        vec![ChatInputImage {
            id: "[Image #1]".to_string(),
            base64: "aW1nZGF0YQ==".to_string(),
            media_type: "image/png".to_string(),
        }],
    );
    message.input_id = Some(input_id.clone());
    let items = vec![message];

    app.update_ui(UiEvent::UserMessagesAdded(items), &ui_tx, &spawn_refs);

    // 占位被清除
    assert!(
        app.model.conversation.queued_submissions.is_empty(),
        "handler 应按 input_id 清占位"
    );

    // 回显文本应含占位符（"看图[Image #1]"）——这是 #507 修复目标
    let user_echo_texts: Vec<&str> = app
        .model
        .conversation
        .timeline
        .items()
        .iter()
        .filter_map(|b| {
            if let crate::tui::model::output_timeline::OutputTimelineItem::UserMessage {
                text,
                ..
            } = b
            {
                Some(text.as_str())
            } else {
                None
            }
        })
        .collect();
    assert_eq!(
        user_echo_texts,
        vec!["看图[Image #1]"],
        "回显应经 message.text_content() 还原含占位符（#507 修复目标）"
    );
}
