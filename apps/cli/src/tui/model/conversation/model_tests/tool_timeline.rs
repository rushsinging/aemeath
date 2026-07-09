#[test]
fn test_conversation_keeps_tool_args_preview() {
    let mut model = ConversationModel::default();
    model.apply(StartChat {
        submission: "read file".to_string(),
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
        id: super::ids::ToolCallId::new("tool-1"),
        provider_id: None,
        name: "Read".to_string(),
        index: 0,
        arguments: Some(r#"{"file_path":"src/main.rs"}"#.to_string()),
        status: ToolCallStatus::Ready,
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

    let chat_id = super::ids::ChatId::new("chat-1");
    let turn_id = super::ids::ChatTurnId::new("turn-1");
    let read_call = tool_call(
        &model,
        &chat_id,
        &turn_id,
        &super::ids::ToolCallId::new("tool-1"),
    )
    .expect("Read tool call should exist");
    assert!(read_call.args_preview.contains("src/main.rs"));
}

#[test]
fn test_tool_call_timeline_item_stores_reference_not_copied_payload() {
    let mut model = ConversationModel::default();
    model.apply(StartChat {
        submission: "read file".to_string(),
    });
    model.apply(ToolCallUpdate {
        chat_id: super::ids::ChatId::new("chat-1"),
        turn_id: super::ids::ChatTurnId::new("turn-1"),
        provider_id: Some("provider-1".to_string()),
        id: super::ids::ToolCallId::new("tool-1"),
        name: "Read".to_string(),
        index: 0,
        arguments: Some(r#"{"file_path":"src/main.rs"}"#.to_string()),
        status: ToolCallStatus::Ready,
    });

    let timeline_item = model
        .timeline
        .items()
        .iter()
        .find(|item| matches!(item, OutputTimelineItem::ToolCall { .. }))
        .expect("timeline should contain tool call ref");

    let chat_id = super::ids::ChatId::new("chat-1");
    let turn_id = super::ids::ChatTurnId::new("turn-1");
    let tool_1_id = super::ids::ToolCallId::new("tool-1");
    match timeline_item {
        OutputTimelineItem::ToolCall { reference } => {
            assert_eq!(reference.context.chat_id, chat_id);
            assert_eq!(reference.context.turn_id, turn_id);
            assert_eq!(reference.tool_call_id, tool_1_id);
        }
        _ => unreachable!(),
    }
    let call = tool_call(&model, &chat_id, &turn_id, &tool_1_id)
        .expect("tool payload should live in chat turn model");
    assert_eq!(call.name, "Read");
    assert!(call.args_preview.contains("src/main.rs"));
}

#[test]
fn test_agent_tool_result_not_orphan_with_index_mismatch() {
    // #95 场景：LLM 返回 text + tool_use 时，ToolCallStart 用纯 tool 序号 (0)，
    // ToolCall 用 content_block index (1)。验证 Agent tool result 不因此变成 orphan。
    let mut model = ConversationModel::default();
    model.apply(StartChat {
        submission: "review code".to_string(),
    });
    // LLM 先输出 assistant text（content_block 0）
    model.apply(AssistantText {
        chat_id: super::ids::ChatId::new("chat-1"),
        turn_id: super::ids::ChatTurnId::new("turn-1"),
        text: "让我来审查".to_string(),
    });
    model.apply(CompleteBlock {
        chat_id: super::ids::ChatId::new("chat-1"),
        turn_id: super::ids::ChatTurnId::new("turn-1"),
    });
    // ToolCallStart 用纯 tool 序号 index=0
    model.apply(ToolCallStart {
        chat_id: super::ids::ChatId::new("chat-1"),
        turn_id: super::ids::ChatTurnId::new("turn-1"),
        id: super::ids::ToolCallId::new("tool-1"),
        provider_id: None,
        name: "Agent".to_string(),
        index: 0,
    });
    // ToolCall 用 content_block index=1（因为 text 占了 block 0）
    model.apply(ToolCallUpdate {
        chat_id: super::ids::ChatId::new("chat-1"),
        turn_id: super::ids::ChatTurnId::new("turn-1"),
        provider_id: Some("provider-1".to_string()),
        id: super::ids::ToolCallId::new("call_agent_1"),
        name: "Agent".to_string(),
        index: 1,
        arguments: None,
        status: ToolCallStatus::Ready,
    });
    // Agent progress（不影响绑定）
    model.apply(RecordAgentProgress {
        chat_id: super::ids::ChatId::new("chat-1"),
        turn_id: super::ids::ChatTurnId::new("turn-1"),
        tool_id: super::ids::ToolCallId::new("call_agent_1"),
        message: "reading files...".to_string(),
    });
    // Agent tool result
    let changes = model.apply(ToolResult {
        chat_id: super::ids::ChatId::new("chat-1"),
        turn_id: super::ids::ChatTurnId::new("turn-1"),
        provider_id: "provider-1".to_string(),
        id: super::ids::ToolCallId::new("call_agent_1"),
        tool_name: "Agent".to_string(),
        output: "审查报告".to_string(),
        content: serde_json::json!({ "text": "test output" }),
        is_error: false,
        image_count: 0,
    });

    // result 不应是 orphan
    assert!(
        !changes
            .iter()
            .any(|c| matches!(c, ConversationChange::OrphanToolResultObserved { .. })),
        "Agent tool result 不应变成 orphan"
    );
    assert!(changes.iter().any(|c| matches!(
        c,
        ConversationChange::ToolCallCompleted { status, .. } if *status == ToolCallStatus::Success
    )));
    assert!(!model.timeline.items().iter().any(|item| matches!(
        item,
        OutputTimelineItem::OrphanToolResult { id, .. } if id == "call_agent_1"
    )));
}

#[test]
fn test_agent_tool_result_not_orphan_text_streaming_then_tool() {
    // #95 场景 B：assistant text 还在 streaming（未 CompleteBlock）时，
    // tool call 就到了。ToolCallStart index=0, ToolCall index=1（错位）。
    let mut model = ConversationModel::default();
    model.apply(StartChat {
        submission: "review".to_string(),
    });
    model.apply(AssistantText {
        chat_id: super::ids::ChatId::new("chat-1"),
        turn_id: super::ids::ChatTurnId::new("turn-1"),
        text: "让我".to_string(),
    });
    // 不调 CompleteBlock — text 还在 streaming
    model.apply(ToolCallStart {
        chat_id: super::ids::ChatId::new("chat-1"),
        turn_id: super::ids::ChatTurnId::new("turn-1"),
        id: super::ids::ToolCallId::new("tool-1"),
        provider_id: None,
        name: "Agent".to_string(),
        index: 0,
    });
    model.apply(ToolCallUpdate {
        chat_id: super::ids::ChatId::new("chat-1"),
        turn_id: super::ids::ChatTurnId::new("turn-1"),
        provider_id: Some("provider-1".to_string()),
        id: super::ids::ToolCallId::new("call_abc"),
        name: "Agent".to_string(),
        index: 1,
        arguments: None,
        status: ToolCallStatus::Ready,
    });
    let changes = model.apply(ToolResult {
        chat_id: super::ids::ChatId::new("chat-1"),
        turn_id: super::ids::ChatTurnId::new("turn-1"),
        provider_id: "provider-1".to_string(),
        id: super::ids::ToolCallId::new("call_abc"),
        tool_name: "Agent".to_string(),
        output: "报告".to_string(),
        content: serde_json::json!({ "text": "test output" }),
        is_error: false,
        image_count: 0,
    });

    assert!(
        !changes
            .iter()
            .any(|c| matches!(c, ConversationChange::OrphanToolResultObserved { .. })),
        "Agent result 不应因 text streaming 而变 orphan"
    );
}

#[test]
fn test_tool_result_not_orphan_when_no_tool_call_start() {
    // #95 核心场景：provider 未发送 ToolCallStart，直接发送 ToolCall + ToolResult。
    // 修复前 observe_tool_call 中 bind_tool 返回 None 导致 ToolCall block 不被创建，
    // ToolResult 到达时 complete_active_tool 找不到匹配 id → orphan。
    let mut model = ConversationModel::default();
    model.apply(StartChat {
        submission: "review code".to_string(),
    });
    // 不发送 ToolCallStart
    model.apply(ToolCallUpdate {
        chat_id: super::ids::ChatId::new("chat-1"),
        turn_id: super::ids::ChatTurnId::new("turn-1"),
        provider_id: Some("provider-1".to_string()),
        id: super::ids::ToolCallId::new("call_agent_no_start"),
        name: "Agent".to_string(),
        index: 0,
        arguments: None,
        status: ToolCallStatus::Ready,
    });
    let changes = model.apply(ToolResult {
        chat_id: super::ids::ChatId::new("chat-1"),
        turn_id: super::ids::ChatTurnId::new("turn-1"),
        provider_id: "provider-1".to_string(),
        id: super::ids::ToolCallId::new("call_agent_no_start"),
        tool_name: "Agent".to_string(),
        output: "审查报告".to_string(),
        content: serde_json::json!({ "text": "test output" }),
        is_error: false,
        image_count: 0,
    });

    assert!(
        !changes
            .iter()
            .any(|c| matches!(c, ConversationChange::OrphanToolResultObserved { .. })),
        "没有 ToolCallStart 时 ToolResult 不应变 orphan（bind_tool 应自动创建占位）"
    );
    assert!(changes.iter().any(|c| matches!(
        c,
        ConversationChange::ToolCallCompleted { status, .. } if *status == ToolCallStatus::Success
    )));
}
