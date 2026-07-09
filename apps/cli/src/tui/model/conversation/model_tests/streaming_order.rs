#[test]
fn test_conversation_streams_text_and_thinking_into_blocks() {
    let mut model = ConversationModel::default();
    model.apply(StartChat {
        submission: "hello".to_string(),
    });
    model.apply(ThinkingText {
        chat_id: super::ids::ChatId::new("chat-1"),
        turn_id: super::ids::ChatTurnId::new("turn-1"),
        text: "plan".to_string(),
    });
    model.apply(AssistantText {
        chat_id: super::ids::ChatId::new("chat-1"),
        turn_id: super::ids::ChatTurnId::new("turn-1"),
        text: "answer".to_string(),
    });

    assert!(model.timeline.items().iter().any(|item| matches!(
        item,
        OutputTimelineItem::Thinking { text, .. } if text == "plan"
    )));
    assert!(model.timeline.items().iter().any(|item| matches!(
        item,
        OutputTimelineItem::AssistantText { text, .. } if text == "answer"
    )));
}

#[test]
fn test_conversation_starts_new_thinking_block_after_block_complete() {
    let mut model = ConversationModel::default();
    model.apply(StartChat {
        submission: "inspect state".to_string(),
    });
    model.apply(ThinkingText {
        chat_id: super::ids::ChatId::new("chat-1"),
        turn_id: super::ids::ChatTurnId::new("turn-1"),
        text: "first thought".to_string(),
    });
    model.apply(CompleteBlock {
        chat_id: super::ids::ChatId::new("chat-1"),
        turn_id: super::ids::ChatTurnId::new("turn-1"),
    });
    model.apply(ThinkingText {
        chat_id: super::ids::ChatId::new("chat-1"),
        turn_id: super::ids::ChatTurnId::new("turn-1"),
        text: "second thought".to_string(),
    });

    let thinking_blocks: Vec<_> = model
        .timeline
        .items()
        .iter()
        .filter_map(|item| match item {
            OutputTimelineItem::Thinking { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect();

    assert_eq!(thinking_blocks, vec!["first thought", "second thought"]);
}

#[test]
fn test_conversation_keeps_live_tool_call_after_preceding_assistant_text() {
    let mut model = ConversationModel::default();
    model.apply(StartChat {
        submission: "check docs".to_string(),
    });
    model.apply(AssistantText {
        chat_id: super::ids::ChatId::new("chat-1"),
        turn_id: super::ids::ChatTurnId::new("turn-1"),
        text: "结论先到".to_string(),
    });
    model.apply(ToolCallStart {
        chat_id: super::ids::ChatId::new("chat-1"),
        turn_id: super::ids::ChatTurnId::new("turn-1"),
        id: super::ids::ToolCallId::new("tool-1"),
        provider_id: None,
        name: "Read".to_string(),
        index: 0,
    });
    model.apply(ToolCallUpdate {
        chat_id: super::ids::ChatId::new("chat-1"),
        turn_id: super::ids::ChatTurnId::new("turn-1"),
        provider_id: Some("provider-1".to_string()),
        id: super::ids::ToolCallId::new("tool-1"),
        name: "Read".to_string(),
        index: 0,
        arguments: None,
        status: ToolCallStatus::Ready,
    });

    let text_pos = model
        .timeline
        .items()
        .iter()
        .position(|item| {
            matches!(
                item,
                OutputTimelineItem::AssistantText { text, .. } if text == "结论先到"
            )
        })
        .expect("assistant text block");
    let tool_1_id = super::ids::ToolCallId::new("tool-1");
    let tool_pos = model
        .timeline
        .items()
        .iter()
        .position(|item| {
            matches!(
                item,
                OutputTimelineItem::ToolCall { reference } if reference.tool_call_id == tool_1_id
            )
        })
        .expect("tool block");

    assert!(
        text_pos < tool_pos,
        "live 场景中后到达的 tool call 应显示在已出现文本之后"
    );
}

#[test]
fn test_conversation_keeps_tool_after_completed_assistant_text() {
    let mut model = ConversationModel::default();
    model.apply(StartChat {
        submission: "check docs".to_string(),
    });
    model.apply(AssistantText {
        chat_id: super::ids::ChatId::new("chat-1"),
        turn_id: super::ids::ChatTurnId::new("turn-1"),
        text: "已经完成的文字".to_string(),
    });
    model.apply(CompleteBlock {
        chat_id: super::ids::ChatId::new("chat-1"),
        turn_id: super::ids::ChatTurnId::new("turn-1"),
    });
    model.apply(ToolCallStart {
        chat_id: super::ids::ChatId::new("chat-1"),
        turn_id: super::ids::ChatTurnId::new("turn-1"),
        id: super::ids::ToolCallId::new("tool-1"),
        provider_id: None,
        name: "Read".to_string(),
        index: 0,
    });
    model.apply(ToolCallUpdate {
        chat_id: super::ids::ChatId::new("chat-1"),
        turn_id: super::ids::ChatTurnId::new("turn-1"),
        provider_id: Some("provider-1".to_string()),
        id: super::ids::ToolCallId::new("tool-1"),
        name: "Read".to_string(),
        index: 0,
        arguments: None,
        status: ToolCallStatus::Ready,
    });

    let tool_1_id = super::ids::ToolCallId::new("tool-1");
    let text_pos = model
        .timeline
        .items()
        .iter()
        .position(|item| {
            matches!(
                item,
                OutputTimelineItem::AssistantText { text, .. } if text == "已经完成的文字"
            )
        })
        .expect("assistant text block");
    let tool_pos = model
        .timeline
        .items()
        .iter()
        .position(|item| {
            matches!(
                item,
                OutputTimelineItem::ToolCall { reference } if reference.tool_call_id == tool_1_id
            )
        })
        .expect("tool block");

    assert!(text_pos < tool_pos, "已完成文本块不应被后续工具调用重排");
}

#[test]
fn test_conversation_places_tool_result_after_late_bound_tool_call() {
    let mut model = ConversationModel::default();
    model.apply(StartChat {
        submission: "read docs".to_string(),
    });
    model.apply(ToolResult {
        chat_id: super::ids::ChatId::new("chat-1"),
        turn_id: super::ids::ChatTurnId::new("turn-1"),
        provider_id: "provider-1".to_string(),
        id: super::ids::ToolCallId::new("tool-1"),
        tool_name: "Read".to_string(),
        output: "file contents".to_string(),
        content: serde_json::json!({ "text": "test output" }),
        is_error: false,
        image_count: 0,
    });
    model.apply(ToolCallStart {
        chat_id: super::ids::ChatId::new("chat-1"),
        turn_id: super::ids::ChatTurnId::new("turn-1"),
        id: super::ids::ToolCallId::new("tool-1"),
        provider_id: None,
        name: "Read".to_string(),
        index: 0,
    });
    model.apply(ToolCallUpdate {
        chat_id: super::ids::ChatId::new("chat-1"),
        turn_id: super::ids::ChatTurnId::new("turn-1"),
        provider_id: Some("provider-1".to_string()),
        id: super::ids::ToolCallId::new("tool-1"),
        name: "Read".to_string(),
        index: 0,
        arguments: None,
        status: ToolCallStatus::Ready,
    });

    let tool_1_id = super::ids::ToolCallId::new("tool-1");
    let positions: Vec<_> = model
        .timeline
        .items()
        .iter()
        .enumerate()
        .filter_map(|(index, item)| match item {
            OutputTimelineItem::ToolCall { reference }
            | OutputTimelineItem::ToolResult { reference }
                if reference.tool_call_id == tool_1_id =>
            {
                Some(index)
            }
            _ => None,
        })
        .collect();

    assert_eq!(positions.len(), 2);
    assert!(
        positions[0] < positions[1],
        "工具结果不应显示在工具标题之前"
    );
}

#[test]
fn test_conversation_keeps_tool_result_after_existing_tool_call() {
    let mut model = ConversationModel::default();
    model.apply(StartChat {
        submission: "read docs".to_string(),
    });
    model.apply(ToolCallStart {
        chat_id: super::ids::ChatId::new("chat-1"),
        turn_id: super::ids::ChatTurnId::new("turn-1"),
        id: super::ids::ToolCallId::new("tool-1"),
        provider_id: None,
        name: "Read".to_string(),
        index: 0,
    });
    model.apply(ToolCallUpdate {
        chat_id: super::ids::ChatId::new("chat-1"),
        turn_id: super::ids::ChatTurnId::new("turn-1"),
        provider_id: Some("provider-1".to_string()),
        id: super::ids::ToolCallId::new("tool-1"),
        name: "Read".to_string(),
        index: 0,
        arguments: None,
        status: ToolCallStatus::Ready,
    });
    model.apply(ToolResult {
        chat_id: super::ids::ChatId::new("chat-1"),
        turn_id: super::ids::ChatTurnId::new("turn-1"),
        provider_id: "provider-1".to_string(),
        id: super::ids::ToolCallId::new("tool-1"),
        tool_name: "Read".to_string(),
        output: "file contents".to_string(),
        content: serde_json::json!({ "text": "test output" }),
        is_error: false,
        image_count: 0,
    });

    let tool_1_id = super::ids::ToolCallId::new("tool-1");
    let positions: Vec<_> = model
        .timeline
        .items()
        .iter()
        .enumerate()
        .filter_map(|(index, item)| match item {
            OutputTimelineItem::ToolCall { reference }
            | OutputTimelineItem::ToolResult { reference }
                if reference.tool_call_id == tool_1_id =>
            {
                Some(index)
            }
            _ => None,
        })
        .collect();

    assert_eq!(positions.len(), 2);
    assert!(positions[0] < positions[1]);
}

