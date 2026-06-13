//! ConversationModel 辅助功能测试（reset、append_user_message）。

use super::block::ConversationBlock;
use super::change::ConversationChange;
use super::ids::{ChatId, ChatTurnId};
use super::intent::ConversationIntent;
use super::model::ConversationModel;
use super::tool_call::ToolCallStatus;

#[test]
fn test_append_user_message_pushes_block_without_new_chat() {
    let mut model = ConversationModel::default();
    model.apply(ConversationIntent::StartChat {
        submission: "ask".to_string(),
    });
    let chats_before = model.chats.len();
    let active_before = model.active_chat_id.clone();

    let changes = model.apply(ConversationIntent::AppendUserMessage {
        text: "我的答复".to_string(),
    });

    // 正常路径：追加 UserMessage 块，但不新开 chat、不改 active_chat_id。
    assert_eq!(model.chats.len(), chats_before, "不应新建 chat");
    assert_eq!(
        model.active_chat_id, active_before,
        "active_chat_id 应保持不变"
    );
    assert!(changes
        .iter()
        .any(|change| matches!(change, ConversationChange::UserMessageAppended { .. })));
    assert!(model.blocks.iter().any(|block| matches!(
        block,
        super::block::ConversationBlock::UserMessage { text, .. } if text == "我的答复"
    )));
}

#[test]
fn test_append_user_message_on_empty_model_creates_block() {
    // 边界：尚无任何 chat 时也能回显（不依赖 active_chat）。
    let mut model = ConversationModel::default();
    model.apply(ConversationIntent::AppendUserMessage {
        text: "孤立回显".to_string(),
    });
    assert!(model.chats.is_empty(), "回显不应创建 chat");
    assert_eq!(model.blocks.len(), 1);
    assert!(model.blocks.iter().any(|block| matches!(
        block,
        super::block::ConversationBlock::UserMessage { text, .. } if text == "孤立回显"
    )));
}

#[test]
fn test_append_user_message_empty_text_still_creates_block() {
    // 错误/边界：空文本仍创建一个 UserMessage 块（渲染层负责显示 "> "）。
    let mut model = ConversationModel::default();
    let changes = model.apply(ConversationIntent::AppendUserMessage {
        text: String::new(),
    });
    assert_eq!(model.blocks.len(), 1);
    assert!(changes
        .iter()
        .any(|change| matches!(change, ConversationChange::OutputDirty)));
}

#[test]
fn test_conversation_reset_clears_all_blocks() {
    let mut model = ConversationModel::default();
    model.apply(ConversationIntent::StartChat {
        submission: "hello".to_string(),
    });
    model.apply(ConversationIntent::AppendSystemMessage {
        text: "note".to_string(),
    });
    assert!(!model.blocks.is_empty());
    assert!(model.active_chat_id.is_some());

    model.reset();

    assert!(model.blocks.is_empty());
    assert!(model.chats.is_empty());
    assert!(model.active_chat_id.is_none());
    assert!(model.queued_submissions.is_empty());
}

#[test]
fn test_conversation_reset_on_empty_is_noop() {
    let mut model = ConversationModel::default();
    model.reset();
    assert!(model.blocks.is_empty());
    assert!(model.active_chat_id.is_none());
}

#[test]
fn test_runtime_tool_event_creates_chat_from_runtime_context_without_active_chat() {
    let mut model = ConversationModel::default();

    let runtime_chat_id = ChatId::new("runtime-chat-1");
    let runtime_turn_id = ChatTurnId::new("runtime-turn-1");
    model.ensure_runtime_turn(runtime_chat_id.clone(), runtime_turn_id.clone());
    let changes = model.apply(ConversationIntent::ObserveToolCallStart {
        chat_id: runtime_chat_id.clone(),
        turn_id: runtime_turn_id.clone(),
        id: "tool-1".to_string(),
        provider_id: None,
        name: "Bash".to_string(),
        index: 0,
    });
    model.apply(ConversationIntent::ObserveToolCallUpdate {
        chat_id: runtime_chat_id,
        turn_id: runtime_turn_id,
        id: "tool-1".to_string(),
        provider_id: None,
        name: "Bash".to_string(),
        index: 0,
        arguments: Some(r#"{"command":"pwd"}"#.to_string()),
        summary: None,
        status: ToolCallStatus::Ready,
    });

    assert!(model.active_chat_id.is_none());
    assert!(changes
        .iter()
        .any(|change| matches!(change, ConversationChange::ToolCallObserved { name, .. } if name == "Bash")));
    assert!(model.blocks.iter().any(|block| matches!(
        block,
        ConversationBlock::ToolCall { id, name, args_preview, .. }
            if id.as_ref() == "tool-1" && name == "Bash" && args_preview.contains("pwd")
    )));
}
