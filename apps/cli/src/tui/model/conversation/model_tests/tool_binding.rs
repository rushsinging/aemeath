#[test]
fn test_conversation_observes_tool_lifecycle() {
    let mut model = ConversationModel::default();
    let changes = model.apply(StartChat {
        submission: "read file".to_string(),
    });
    assert!(changes
        .iter()
        .any(|change| matches!(change, ConversationChange::ChatStarted { .. })));

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
    let changes = model.apply(ToolResult {
        chat_id: super::ids::ChatId::new("chat-1"),
        turn_id: super::ids::ChatTurnId::new("turn-1"),
        provider_id: "provider-1".to_string(),
        id: super::ids::ToolCallId::new("tool-1"),
        tool_name: "Read".to_string(),
        output: "ok".to_string(),
        content: serde_json::json!({ "text": "test output" }),
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
    model.apply(StartChat {
        submission: "read file".to_string(),
    });
    let missing_id = super::ids::ToolCallId::new("missing");
    let changes = model.apply(ToolResult {
        chat_id: super::ids::ChatId::new("chat-1"),
        turn_id: super::ids::ChatTurnId::new("turn-1"),
        provider_id: "provider-1".to_string(),
        id: missing_id.clone(),
        tool_name: "Read".to_string(),
        output: "late".to_string(),
        content: serde_json::json!({ "text": "test output" }),
        is_error: false,
        image_count: 0,
    });
    assert!(changes.iter().any(|change| matches!(
        change,
        ConversationChange::OrphanToolResultObserved { id } if *id == missing_id.to_string()
    )));
}

#[test]
fn test_conversation_reused_runtime_ids_across_turns_do_not_overwrite_earlier_blocks() {
    let mut model = ConversationModel::default();
    model.apply(StartChat {
        submission: "load first skill".to_string(),
    });
    model.apply(ToolCallStart {
        chat_id: super::ids::ChatId::new("chat-1"),
        turn_id: super::ids::ChatTurnId::new("turn-1"),
        id: super::ids::ToolCallId::new("tool-1"),
        provider_id: None,
        name: "Skill".to_string(),
        index: 0,
    });
    model.apply(ToolCallUpdate {
        chat_id: super::ids::ChatId::new("chat-1"),
        turn_id: super::ids::ChatTurnId::new("turn-1"),
        id: super::ids::ToolCallId::new("tool-1"),
        provider_id: None,
        name: "Skill".to_string(),
        index: 0,
        arguments: Some(r#"{"skill":"superpowers:using-superpowers"}"#.to_string()),
        status: ToolCallStatus::Ready,
    });
    model.apply(ToolCallUpdate {
        chat_id: super::ids::ChatId::new("chat-1"),
        turn_id: super::ids::ChatTurnId::new("turn-1"),
        provider_id: Some("call-using".to_string()),
        id: super::ids::ToolCallId::new("tool-1"),
        name: "Skill".to_string(),
        index: 0,
        arguments: None,
        status: ToolCallStatus::Ready,
    });
    model.apply(CompleteChat {
        chat_id: super::ids::ChatId::new("chat-1"),
        turn_id: super::ids::ChatTurnId::new("turn-1"),
    });

    model.apply(StartChat {
        submission: "load second skill".to_string(),
    });
    model.apply(ToolCallStart {
        chat_id: super::ids::ChatId::new("chat-1"),
        turn_id: super::ids::ChatTurnId::new("turn-1"),
        id: super::ids::ToolCallId::new("tool-3"),
        provider_id: None,
        name: "Skill".to_string(),
        index: 0,
    });
    model.apply(ToolCallUpdate {
        chat_id: super::ids::ChatId::new("chat-1"),
        turn_id: super::ids::ChatTurnId::new("turn-1"),
        id: super::ids::ToolCallId::new("tool-3"),
        provider_id: None,
        name: "Skill".to_string(),
        index: 0,
        arguments: Some(r#"{"skill":"superpowers:brainstorming"}"#.to_string()),
        status: ToolCallStatus::Ready,
    });
    model.apply(ToolCallUpdate {
        chat_id: super::ids::ChatId::new("chat-1"),
        turn_id: super::ids::ChatTurnId::new("turn-1"),
        provider_id: Some("call-brainstorm".to_string()),
        id: super::ids::ToolCallId::new("tool-3"),
        name: "Skill".to_string(),
        index: 0,
        arguments: None,
        status: ToolCallStatus::Ready,
    });

    let chat_id = super::ids::ChatId::new("chat-1");
    let turn_id = super::ids::ChatTurnId::new("turn-1");
    let chat = model.chats.iter().find(|c| c.id == chat_id).unwrap();
    let turn = chat.turns.iter().find(|t| t.id == turn_id).unwrap();
    let summaries: Vec<_> = turn
        .tool_calls
        .iter()
        .filter(|call| call.name == "Skill")
        .map(|call| call.args_preview.as_str())
        .collect();

    assert_eq!(summaries.len(), 2);
    assert!(summaries[0].contains("superpowers:using-superpowers"));
    assert!(summaries[1].contains("superpowers:brainstorming"));
}

#[test]
fn test_conversation_observe_tool_events_use_explicit_runtime_context_when_active_turn_drifted() {
    let mut model = ConversationModel::default();
    let live_chat = super::ids::ChatId::new("session-live");
    let live_turn = super::ids::ChatTurnId::new("turn-2");
    let stale_chat = super::ids::ChatId::new("session-stale");
    let stale_turn = super::ids::ChatTurnId::new("turn-55");

    model.ensure_runtime_turn(live_chat.clone(), live_turn.clone());
    model.ensure_runtime_turn(stale_chat, stale_turn);

    model.apply(ToolCallStart {
        chat_id: live_chat.clone(),
        turn_id: live_turn.clone(),
        id: super::ids::ToolCallId::new("tool-1"),
        provider_id: Some("call-read".to_string()),
        name: "Read".to_string(),
        index: 0,
    });
    model.apply(ToolCallUpdate {
        chat_id: live_chat.clone(),
        turn_id: live_turn.clone(),
        id: super::ids::ToolCallId::new("tool-1"),
        provider_id: Some("call-read".to_string()),
        name: "Read".to_string(),
        index: 0,
        arguments: Some(r#"{"file_path":"Cargo.toml"}"#.to_string()),
        status: ToolCallStatus::Ready,
    });
    model.apply(ToolResult {
        chat_id: live_chat.clone(),
        turn_id: live_turn.clone(),
        id: super::ids::ToolCallId::new("tool-1"),
        provider_id: "call-read".to_string(),
        tool_name: "Read".to_string(),
        output: "workspace manifest".to_string(),
        content: serde_json::json!({ "text": "workspace manifest" }),
        is_error: false,
        image_count: 0,
    });

    let live_turn_model = model
        .chats
        .iter()
        .find(|chat| chat.id == live_chat)
        .and_then(|chat| chat.turns.iter().find(|turn| turn.id == live_turn))
        .expect("live runtime turn should exist");
    assert_eq!(live_turn_model.tool_calls.len(), 1);
    assert_eq!(
        live_turn_model.tool_calls[0]
            .result
            .as_ref()
            .map(|p| p.output.as_str()),
        Some("workspace manifest")
    );
    let tool_id = super::ids::ToolCallId::new("tool-1");
    let live_call =
        tool_call(&model, &live_chat, &live_turn, &tool_id).expect("live tool call should exist");
    assert_eq!(live_call.name, "Read");
    assert!(live_call.args_preview.contains("Cargo.toml"));
    assert!(timeline_tool_call_ref_exists(
        &model, &live_chat, &live_turn, &tool_id
    ));
    assert!(model.timeline.items().iter().any(|item| matches!(
        item,
        OutputTimelineItem::ToolResult { reference, .. }
            if reference.context.chat_id == live_chat
                && reference.context.turn_id == live_turn
                && reference.tool_call_id == tool_id
    )));
}

#[test]
fn test_conversation_repeated_runtime_id_result_does_not_complete_previous_provider_tool() {
    let mut model = ConversationModel::default();
    model.apply(StartChat {
        submission: "load skill".to_string(),
    });
    model.apply(ToolCallStart {
        chat_id: super::ids::ChatId::new("chat-1"),
        turn_id: super::ids::ChatTurnId::new("turn-1"),
        id: super::ids::ToolCallId::new("tool-1"),
        provider_id: Some("call-skill".to_string()),
        name: "Skill".to_string(),
        index: 0,
    });
    model.apply(ToolCallUpdate {
        chat_id: super::ids::ChatId::new("chat-1"),
        turn_id: super::ids::ChatTurnId::new("turn-1"),
        id: super::ids::ToolCallId::new("tool-1"),
        provider_id: Some("call-skill".to_string()),
        name: "Skill".to_string(),
        index: 0,
        arguments: Some(r#"{"skill":"superpowers:using-superpowers"}"#.to_string()),
        status: ToolCallStatus::Ready,
    });
    model.apply(ToolCallUpdate {
        chat_id: super::ids::ChatId::new("chat-1"),
        turn_id: super::ids::ChatTurnId::new("turn-1"),
        provider_id: Some("call-skill".to_string()),
        id: super::ids::ToolCallId::new("tool-1"),
        name: "Skill".to_string(),
        index: 0,
        arguments: None,
        status: ToolCallStatus::Ready,
    });
    model.apply(CompleteChat {
        chat_id: super::ids::ChatId::new("chat-1"),
        turn_id: super::ids::ChatTurnId::new("turn-1"),
    });

    model.apply(StartChat {
        submission: "read config".to_string(),
    });
    model.apply(ToolResult {
        chat_id: super::ids::ChatId::new("chat-1"),
        turn_id: super::ids::ChatTurnId::new("turn-1"),
        id: super::ids::ToolCallId::new("tool-2"),
        provider_id: "call-read".to_string(),
        tool_name: "Read".to_string(),
        output: "//! Configuration file management".to_string(),
        content: serde_json::json!({ "text": "test output" }),
        is_error: false,
        image_count: 0,
    });
    model.apply(ToolCallUpdate {
        chat_id: super::ids::ChatId::new("chat-1"),
        turn_id: super::ids::ChatTurnId::new("turn-1"),
        provider_id: Some("call-read".to_string()),
        id: super::ids::ToolCallId::new("tool-2"),
        name: "Read".to_string(),
        index: 0,
        arguments: None,
        status: ToolCallStatus::Ready,
    });

    let chat_id = super::ids::ChatId::new("chat-1");
    let turn_id = super::ids::ChatTurnId::new("turn-1");
    let chat = model.chats.iter().find(|c| c.id == chat_id).unwrap();
    let turn = chat.turns.iter().find(|t| t.id == turn_id).unwrap();
    let skill_result = turn.tool_calls[0]
        .result
        .as_ref()
        .map(|p| p.output.as_str());
    assert_ne!(
        skill_result,
        Some("//! Configuration file management"),
        "Read 结果不应写入上一轮 Skill"
    );
    let chat_id = super::ids::ChatId::new("chat-1");
    let turn_id = super::ids::ChatTurnId::new("turn-1");
    let read_call = tool_call(
        &model,
        &chat_id,
        &turn_id,
        &super::ids::ToolCallId::new("tool-2"),
    )
    .expect("Read tool call should exist");
    assert_eq!(read_call.name, "Read");
    assert!(timeline_tool_call_ref_exists(
        &model,
        &chat_id,
        &turn_id,
        &super::ids::ToolCallId::new("tool-2")
    ));
    assert!(read_call
        .result
        .as_ref()
        .is_some_and(|p| p.output.contains("Configuration file management")));
}

#[test]
fn test_conversation_binds_tool_call_by_provider_id_when_runtime_id_changed() {
    let mut model = ConversationModel::default();
    model.apply(StartChat {
        submission: "load skill".to_string(),
    });
    model.apply(ToolCallStart {
        chat_id: super::ids::ChatId::new("chat-1"),
        turn_id: super::ids::ChatTurnId::new("turn-1"),
        id: super::ids::ToolCallId::new("call-provider-skill"),
        provider_id: Some("call-provider-skill".to_string()),
        name: "Skill".to_string(),
        index: 0,
    });
    model.apply(ToolCallUpdate {
        chat_id: super::ids::ChatId::new("chat-1"),
        turn_id: super::ids::ChatTurnId::new("turn-1"),
        id: super::ids::ToolCallId::new("call-provider-skill"),
        provider_id: Some("call-provider-skill".to_string()),
        name: "Skill".to_string(),
        index: 0,
        arguments: Some(r#"{"skill":"superpowers:brainstorming"}"#.to_string()),
        status: ToolCallStatus::Ready,
    });
    model.apply(ToolCallUpdate {
        chat_id: super::ids::ChatId::new("chat-1"),
        turn_id: super::ids::ChatTurnId::new("turn-1"),
        provider_id: Some("call-provider-skill".to_string()),
        id: super::ids::ToolCallId::new("tool-99"),
        name: "Skill".to_string(),
        index: 0,
        arguments: None,
        status: ToolCallStatus::Ready,
    });

    let chat_id = super::ids::ChatId::new("chat-1");
    let turn_id = super::ids::ChatTurnId::new("turn-1");
    let chat = model.chats.iter().find(|c| c.id == chat_id).unwrap();
    let turn = chat.turns.iter().find(|t| t.id == turn_id).unwrap();
    let provider_skill_id = super::ids::ToolCallId::new("call-provider-skill");
    let tool_calls: Vec<_> = turn
        .tool_calls
        .iter()
        .map(|call| {
            (
                call.id.as_ref().unwrap().as_ref(),
                call.args_preview.as_str(),
            )
        })
        .collect();

    assert_eq!(tool_calls.len(), 1);
    assert_eq!(tool_calls[0].0, provider_skill_id.to_string());
    assert!(tool_calls[0].1.contains("superpowers:brainstorming"));
}
#[test]
fn test_conversation_late_tool_call_binds_existing_result() {
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
    model.apply(ToolResult {
        chat_id: super::ids::ChatId::new("chat-1"),
        turn_id: super::ids::ChatTurnId::new("turn-1"),
        provider_id: "provider-1".to_string(),
        id: super::ids::ToolCallId::new("tool-1"),
        tool_name: "Read".to_string(),
        output: "line1\nline2".to_string(),
        content: serde_json::json!({ "text": "test output" }),
        is_error: false,
        image_count: 0,
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
    assert!(!model.timeline.items().iter().any(|item| matches!(
        item,
        OutputTimelineItem::OrphanToolResult { id, .. } if id == "tool-1"
    )));
    assert!(model.timeline.items().iter().any(|item| matches!(
        item,
        OutputTimelineItem::ToolResult { reference, .. } if reference.tool_call_id == tool_1_id
    )));
    assert_eq!(
        model
            .chats
            .iter()
            .find(|c| c.id == super::ids::ChatId::new("chat-1"))
            .unwrap()
            .turns
            .iter()
            .find(|t| t.id == super::ids::ChatTurnId::new("turn-1"))
            .unwrap()
            .tool_calls[0]
            .result
            .as_ref()
            .map(|p| p.output.as_str()),
        Some("line1\nline2")
    );
    assert_eq!(
        model
            .chats
            .iter()
            .find(|c| c.id == super::ids::ChatId::new("chat-1"))
            .unwrap()
            .turns
            .iter()
            .find(|t| t.id == super::ids::ChatTurnId::new("turn-1"))
            .unwrap()
            .tool_calls[0]
            .status,
        ToolCallStatus::Success
    );
}
