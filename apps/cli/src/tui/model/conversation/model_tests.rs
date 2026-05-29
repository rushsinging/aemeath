use super::change::ConversationChange;
use super::intent::ConversationIntent;
use super::model::ConversationModel;
use super::tool_call::ToolCallStatus;

#[test]
fn test_conversation_observes_tool_lifecycle() {
    let mut model = ConversationModel::default();
    let changes = model.apply(ConversationIntent::StartChat {
        submission: "read file".to_string(),
    });
    assert!(changes
        .iter()
        .any(|change| matches!(change, ConversationChange::ChatStarted { .. })));

    model.apply(ConversationIntent::ObserveToolCallStart {
        name: "Read".to_string(),
        index: 0,
    });
    model.apply(ConversationIntent::ObserveToolCall {
        id: "tool-1".to_string(),
        name: "Read".to_string(),
        index: 0,
        summary: "Read file".to_string(),
    });
    let changes = model.apply(ConversationIntent::ObserveToolResult {
        id: "tool-1".to_string(),
        tool_name: "Read".to_string(),
        output: "ok".to_string(),
        is_error: false,
        image_count: 0,
    });

    assert!(changes.iter().any(|change| matches!(
        change,
        ConversationChange::ToolCallCompleted { status, .. } if *status == ToolCallStatus::Success
    )));
}

#[test]
fn test_conversation_reports_orphan_tool_result() {
    let mut model = ConversationModel::default();
    model.apply(ConversationIntent::StartChat {
        submission: "read file".to_string(),
    });
    let changes = model.apply(ConversationIntent::ObserveToolResult {
        id: "missing".to_string(),
        tool_name: "Read".to_string(),
        output: "late".to_string(),
        is_error: false,
        image_count: 0,
    });
    assert!(changes.iter().any(|change| matches!(
        change,
        ConversationChange::OrphanToolResultObserved { id } if id == "missing"
    )));
}

#[test]
fn test_conversation_streams_text_and_thinking_into_blocks() {
    let mut model = ConversationModel::default();
    model.apply(ConversationIntent::StartChat {
        submission: "hello".to_string(),
    });
    model.apply(ConversationIntent::ObserveThinkingText {
        text: "plan".to_string(),
    });
    model.apply(ConversationIntent::ObserveAssistantText {
        text: "answer".to_string(),
    });

    assert!(model.blocks.iter().any(|block| matches!(
        block,
        super::block::ConversationBlock::Thinking { text, .. } if text == "plan"
    )));
    assert!(model.blocks.iter().any(|block| matches!(
        block,
        super::block::ConversationBlock::AssistantText { text, .. } if text == "answer"
    )));
}

#[test]
fn test_conversation_keeps_tool_args_preview() {
    let mut model = ConversationModel::default();
    model.apply(ConversationIntent::StartChat {
        submission: "read file".to_string(),
    });
    model.apply(ConversationIntent::ObserveToolCallStart {
        name: "Read".to_string(),
        index: 0,
    });
    model.apply(ConversationIntent::ObserveToolArguments {
        name: "Read".to_string(),
        index: 0,
        partial_args: r#"{"file_path":"src/main.rs"}"#.to_string(),
    });
    model.apply(ConversationIntent::ObserveToolCall {
        id: "tool-1".to_string(),
        name: "Read".to_string(),
        index: 0,
        summary: "Read file".to_string(),
    });

    assert!(model.blocks.iter().any(|block| matches!(
        block,
        super::block::ConversationBlock::ToolCall { args_preview, .. }
            if args_preview.contains("src/main.rs")
    )));
}

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
    assert_eq!(model.active_chat_id, active_before, "active_chat_id 应保持不变");
    assert!(changes.iter().any(|change| matches!(
        change,
        ConversationChange::UserMessageAppended { .. }
    )));
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
fn test_conversation_reset_allows_fresh_chat_afterwards() {
    let mut model = ConversationModel::default();
    model.apply(ConversationIntent::StartChat {
        submission: "first".to_string(),
    });
    model.reset();
    model.apply(ConversationIntent::AppendSystemMessage {
        text: "[conversation cleared]".to_string(),
    });
    assert_eq!(model.blocks.len(), 1);
    assert!(model.blocks.iter().any(|block| matches!(
        block,
        super::block::ConversationBlock::System { text, .. } if text == "[conversation cleared]"
    )));
}
