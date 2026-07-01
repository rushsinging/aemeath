use super::change::ConversationChange;
use super::intent::ConversationIntent;
use super::model::ConversationModel;
use super::tool_call::ToolCallStatus;
use crate::tui::model::conversation::intent::*;
use crate::tui::model::output_timeline::OutputTimelineItem;

fn tool_call<'a>(
    model: &'a ConversationModel,
    chat_id: &super::ids::ChatId,
    turn_id: &super::ids::ChatTurnId,
    id: &super::ids::ToolCallId,
) -> Option<&'a super::tool_call::ToolCall> {
    model
        .chats
        .iter()
        .find(|chat| &chat.id == chat_id)
        .and_then(|chat| chat.turns.iter().find(|turn| &turn.id == turn_id))
        .and_then(|turn| {
            turn.tool_calls
                .iter()
                .find(|call| call.id.as_ref() == Some(id))
        })
}

fn timeline_tool_call_ref_exists(
    model: &ConversationModel,
    chat_id: &super::ids::ChatId,
    turn_id: &super::ids::ChatTurnId,
    id: &super::ids::ToolCallId,
) -> bool {
    model.timeline.items().iter().any(|item| {
        matches!(
            item,
            OutputTimelineItem::ToolCall { reference }
                if &reference.context.chat_id == chat_id
                    && &reference.context.turn_id == turn_id
                    && reference.tool_call_id == *id
        )
    })
}

#[test]
fn test_ensure_runtime_turn_does_not_change_active_chat() {
    let mut model = ConversationModel::default();
    model.apply(StartChat {
        submission: "user focused chat".to_string(),
    });
    let active_before = model.active_chat_id.clone();

    model.ensure_runtime_turn(
        super::ids::ChatId::new("runtime-chat"),
        super::ids::ChatTurnId::new("runtime-turn"),
    );

    assert_eq!(model.active_chat_id, active_before);
    assert!(model
        .chats
        .iter()
        .any(|chat| chat.id == super::ids::ChatId::new("runtime-chat")));
}

#[test]
fn test_record_agent_progress_uses_explicit_runtime_context_when_active_turn_drifted() {
    let mut model = ConversationModel::default();
    let live_chat = super::ids::ChatId::new("session-live");
    let live_turn = super::ids::ChatTurnId::new("turn-live");
    let stale_chat = super::ids::ChatId::new("session-stale");
    let stale_turn = super::ids::ChatTurnId::new("turn-stale");

    model.ensure_runtime_turn(live_chat.clone(), live_turn.clone());
    let agent_tool_id = super::ids::ToolCallId::new("agent-tool");
    model.apply(ObserveToolCallStart {
        chat_id: live_chat.clone(),
        turn_id: live_turn.clone(),
        id: agent_tool_id.clone(),
        provider_id: Some("provider-agent".to_string()),
        name: "Agent".to_string(),
        index: 0,
    });
    model.ensure_runtime_turn(stale_chat.clone(), stale_turn.clone());

    model.apply(RecordAgentProgress {
        chat_id: live_chat.clone(),
        turn_id: live_turn.clone(),
        tool_id: agent_tool_id.clone(),
        message: "reading files".to_string(),
    });

    let live_call = model
        .chats
        .iter()
        .find(|chat| chat.id == live_chat)
        .and_then(|chat| chat.turns.iter().find(|turn| turn.id == live_turn))
        .and_then(|turn| {
            turn.tool_calls
                .iter()
                .find(|call| call.id.as_ref() == Some(&agent_tool_id))
        })
        .expect("live agent tool call should exist");

    assert_eq!(live_call.activities, vec!["reading files".to_string()]);
}

#[test]
fn test_complete_chat_uses_explicit_runtime_context_when_active_chat_drifted() {
    let mut model = ConversationModel::default();
    let live_chat = super::ids::ChatId::new("session-live");
    let live_turn = super::ids::ChatTurnId::new("turn-live");
    let stale_chat = super::ids::ChatId::new("session-stale");
    let stale_turn = super::ids::ChatTurnId::new("turn-stale");

    model.ensure_runtime_turn(live_chat.clone(), live_turn.clone());
    model.ensure_runtime_turn(stale_chat.clone(), stale_turn);
    model.active_chat_id = Some(stale_chat.clone());

    let changes = model.apply(CompleteChat {
        chat_id: live_chat.clone(),
        turn_id: live_turn,
    });

    let live = model
        .chats
        .iter()
        .find(|chat| chat.id == live_chat)
        .expect("live chat exists");
    let stale = model
        .chats
        .iter()
        .find(|chat| chat.id == stale_chat)
        .expect("stale chat exists");

    assert_eq!(live.status, super::chat::ChatStatus::Completing);
    assert_ne!(stale.status, super::chat::ChatStatus::Completing);
    assert_eq!(model.active_chat_id, Some(stale_chat));
    assert!(matches!(
        changes.as_slice(),
        [ConversationChange::ChatCompleting { chat_id }] if *chat_id == live_chat.to_string()
    ));
}

#[test]
fn test_conversation_observes_tool_lifecycle() {
    let mut model = ConversationModel::default();
    let changes = model.apply(StartChat {
        submission: "read file".to_string(),
    });
    assert!(changes
        .iter()
        .any(|change| matches!(change, ConversationChange::ChatStarted { .. })));

    model.apply(ObserveToolCallStart {
        chat_id: super::ids::ChatId::new("chat-1"),
        turn_id: super::ids::ChatTurnId::new("turn-1"),
        id: super::ids::ToolCallId::new("tool-1"),
        provider_id: None,
        name: "Read".to_string(),
        index: 0,
    });
    model.apply(ObserveToolCallUpdate {
        chat_id: super::ids::ChatId::new("chat-1"),
        turn_id: super::ids::ChatTurnId::new("turn-1"),
        provider_id: Some("provider-1".to_string()),
        id: super::ids::ToolCallId::new("tool-1"),
        name: "Read".to_string(),
        index: 0,
        arguments: None,
        status: ToolCallStatus::Ready,
    });
    let changes = model.apply(ObserveToolResult {
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
    let changes = model.apply(ObserveToolResult {
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
    model.apply(ObserveToolCallStart {
        chat_id: super::ids::ChatId::new("chat-1"),
        turn_id: super::ids::ChatTurnId::new("turn-1"),
        id: super::ids::ToolCallId::new("tool-1"),
        provider_id: None,
        name: "Skill".to_string(),
        index: 0,
    });
    model.apply(ObserveToolCallUpdate {
        chat_id: super::ids::ChatId::new("chat-1"),
        turn_id: super::ids::ChatTurnId::new("turn-1"),
        id: super::ids::ToolCallId::new("tool-1"),
        provider_id: None,
        name: "Skill".to_string(),
        index: 0,
        arguments: Some(r#"{"skill":"superpowers:using-superpowers"}"#.to_string()),
        status: ToolCallStatus::Ready,
    });
    model.apply(ObserveToolCallUpdate {
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
    model.apply(ObserveToolCallStart {
        chat_id: super::ids::ChatId::new("chat-1"),
        turn_id: super::ids::ChatTurnId::new("turn-1"),
        id: super::ids::ToolCallId::new("tool-3"),
        provider_id: None,
        name: "Skill".to_string(),
        index: 0,
    });
    model.apply(ObserveToolCallUpdate {
        chat_id: super::ids::ChatId::new("chat-1"),
        turn_id: super::ids::ChatTurnId::new("turn-1"),
        id: super::ids::ToolCallId::new("tool-3"),
        provider_id: None,
        name: "Skill".to_string(),
        index: 0,
        arguments: Some(r#"{"skill":"superpowers:brainstorming"}"#.to_string()),
        status: ToolCallStatus::Ready,
    });
    model.apply(ObserveToolCallUpdate {
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

    model.apply(ObserveToolCallStart {
        chat_id: live_chat.clone(),
        turn_id: live_turn.clone(),
        id: super::ids::ToolCallId::new("tool-1"),
        provider_id: Some("call-read".to_string()),
        name: "Read".to_string(),
        index: 0,
    });
    model.apply(ObserveToolCallUpdate {
        chat_id: live_chat.clone(),
        turn_id: live_turn.clone(),
        id: super::ids::ToolCallId::new("tool-1"),
        provider_id: Some("call-read".to_string()),
        name: "Read".to_string(),
        index: 0,
        arguments: Some(r#"{"file_path":"Cargo.toml"}"#.to_string()),
        status: ToolCallStatus::Ready,
    });
    model.apply(ObserveToolResult {
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
    model.apply(ObserveToolCallStart {
        chat_id: super::ids::ChatId::new("chat-1"),
        turn_id: super::ids::ChatTurnId::new("turn-1"),
        id: super::ids::ToolCallId::new("tool-1"),
        provider_id: Some("call-skill".to_string()),
        name: "Skill".to_string(),
        index: 0,
    });
    model.apply(ObserveToolCallUpdate {
        chat_id: super::ids::ChatId::new("chat-1"),
        turn_id: super::ids::ChatTurnId::new("turn-1"),
        id: super::ids::ToolCallId::new("tool-1"),
        provider_id: Some("call-skill".to_string()),
        name: "Skill".to_string(),
        index: 0,
        arguments: Some(r#"{"skill":"superpowers:using-superpowers"}"#.to_string()),
        status: ToolCallStatus::Ready,
    });
    model.apply(ObserveToolCallUpdate {
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
    model.apply(ObserveToolResult {
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
    model.apply(ObserveToolCallUpdate {
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
    model.apply(ObserveToolCallStart {
        chat_id: super::ids::ChatId::new("chat-1"),
        turn_id: super::ids::ChatTurnId::new("turn-1"),
        id: super::ids::ToolCallId::new("call-provider-skill"),
        provider_id: Some("call-provider-skill".to_string()),
        name: "Skill".to_string(),
        index: 0,
    });
    model.apply(ObserveToolCallUpdate {
        chat_id: super::ids::ChatId::new("chat-1"),
        turn_id: super::ids::ChatTurnId::new("turn-1"),
        id: super::ids::ToolCallId::new("call-provider-skill"),
        provider_id: Some("call-provider-skill".to_string()),
        name: "Skill".to_string(),
        index: 0,
        arguments: Some(r#"{"skill":"superpowers:brainstorming"}"#.to_string()),
        status: ToolCallStatus::Ready,
    });
    model.apply(ObserveToolCallUpdate {
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
    model.apply(ObserveToolCallStart {
        chat_id: super::ids::ChatId::new("chat-1"),
        turn_id: super::ids::ChatTurnId::new("turn-1"),
        id: super::ids::ToolCallId::new("tool-1"),
        provider_id: None,
        name: "Read".to_string(),
        index: 0,
    });
    model.apply(ObserveToolResult {
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
    model.apply(ObserveToolCallUpdate {
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

#[test]
fn test_conversation_streams_text_and_thinking_into_blocks() {
    let mut model = ConversationModel::default();
    model.apply(StartChat {
        submission: "hello".to_string(),
    });
    model.apply(ObserveThinkingText {
        chat_id: super::ids::ChatId::new("chat-1"),
        turn_id: super::ids::ChatTurnId::new("turn-1"),
        text: "plan".to_string(),
    });
    model.apply(ObserveAssistantText {
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
    model.apply(ObserveThinkingText {
        chat_id: super::ids::ChatId::new("chat-1"),
        turn_id: super::ids::ChatTurnId::new("turn-1"),
        text: "first thought".to_string(),
    });
    model.apply(CompleteBlock {
        chat_id: super::ids::ChatId::new("chat-1"),
        turn_id: super::ids::ChatTurnId::new("turn-1"),
    });
    model.apply(ObserveThinkingText {
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
    model.apply(ObserveAssistantText {
        chat_id: super::ids::ChatId::new("chat-1"),
        turn_id: super::ids::ChatTurnId::new("turn-1"),
        text: "结论先到".to_string(),
    });
    model.apply(ObserveToolCallStart {
        chat_id: super::ids::ChatId::new("chat-1"),
        turn_id: super::ids::ChatTurnId::new("turn-1"),
        id: super::ids::ToolCallId::new("tool-1"),
        provider_id: None,
        name: "Read".to_string(),
        index: 0,
    });
    model.apply(ObserveToolCallUpdate {
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
    model.apply(ObserveAssistantText {
        chat_id: super::ids::ChatId::new("chat-1"),
        turn_id: super::ids::ChatTurnId::new("turn-1"),
        text: "已经完成的文字".to_string(),
    });
    model.apply(CompleteBlock {
        chat_id: super::ids::ChatId::new("chat-1"),
        turn_id: super::ids::ChatTurnId::new("turn-1"),
    });
    model.apply(ObserveToolCallStart {
        chat_id: super::ids::ChatId::new("chat-1"),
        turn_id: super::ids::ChatTurnId::new("turn-1"),
        id: super::ids::ToolCallId::new("tool-1"),
        provider_id: None,
        name: "Read".to_string(),
        index: 0,
    });
    model.apply(ObserveToolCallUpdate {
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
    model.apply(ObserveToolResult {
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
    model.apply(ObserveToolCallStart {
        chat_id: super::ids::ChatId::new("chat-1"),
        turn_id: super::ids::ChatTurnId::new("turn-1"),
        id: super::ids::ToolCallId::new("tool-1"),
        provider_id: None,
        name: "Read".to_string(),
        index: 0,
    });
    model.apply(ObserveToolCallUpdate {
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
    model.apply(ObserveToolCallStart {
        chat_id: super::ids::ChatId::new("chat-1"),
        turn_id: super::ids::ChatTurnId::new("turn-1"),
        id: super::ids::ToolCallId::new("tool-1"),
        provider_id: None,
        name: "Read".to_string(),
        index: 0,
    });
    model.apply(ObserveToolCallUpdate {
        chat_id: super::ids::ChatId::new("chat-1"),
        turn_id: super::ids::ChatTurnId::new("turn-1"),
        provider_id: Some("provider-1".to_string()),
        id: super::ids::ToolCallId::new("tool-1"),
        name: "Read".to_string(),
        index: 0,
        arguments: None,
        status: ToolCallStatus::Ready,
    });
    model.apply(ObserveToolResult {
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

#[test]
fn test_queue_submission_pushes_queued_user_message_block() {
    // 正常路径：排队提交经 ConversationModel 进入 QueuedUserMessage 块（取代旧
    // OutputArea::queued_messages 命令式显示路径）。
    let mut model = ConversationModel::default();
    let changes = model.apply(QueueSubmission {
        input_id: sdk::InputId::new_v7(),
        text: "排队的消息".to_string(),
    });

    assert!(changes
        .iter()
        .any(|c| matches!(c, ConversationChange::QueuedSubmissionAdded { .. })));
    assert!(model.timeline.items().iter().any(|item| matches!(
        item,
        OutputTimelineItem::QueuedUserMessage { text, .. } if text == "排队的消息"
    )));
    assert_eq!(model.queued_submissions.len(), 1);
}

#[test]
fn test_clear_queued_by_id_removes_only_matching_entry() {
    // 入队 3 条占位（A/B/C），按 B 的 input_id 精确清除后，
    // queued_submissions / blocks / timeline 三处各只剩 A 和 C。
    let mut model = ConversationModel::default();
    let id_a = sdk::InputId::new_v7();
    let id_b = sdk::InputId::new_v7();
    let id_c = sdk::InputId::new_v7();

    model.apply(QueueSubmission {
        input_id: id_a.clone(),
        text: "A".to_string(),
    });
    model.apply(QueueSubmission {
        input_id: id_b.clone(),
        text: "B".to_string(),
    });
    model.apply(QueueSubmission {
        input_id: id_c.clone(),
        text: "C".to_string(),
    });

    let changes = model.apply(ClearQueuedSubmissionById {
        input_id: id_b.clone(),
    });

    // 只移除了 1 条
    assert!(changes.iter().any(|c| matches!(
        c,
        ConversationChange::QueuedSubmissionsCleared { count } if *count == 1
    )));

    // queued_submissions：剩 A、C，无 B
    assert_eq!(model.queued_submissions.len(), 2);
    assert!(model.queued_submissions.iter().any(|q| q.input_id == id_a));
    assert!(model.queued_submissions.iter().any(|q| q.input_id == id_c));
    assert!(!model.queued_submissions.iter().any(|q| q.input_id == id_b));

    // timeline：剩 A、C 的 QueuedUserMessage，无 B
    let queued_timeline: Vec<_> = model
        .timeline
        .items()
        .iter()
        .filter_map(|it| match it {
            OutputTimelineItem::QueuedUserMessage { input_id, text, .. } => {
                Some((input_id.clone(), text.clone()))
            }
            _ => None,
        })
        .collect();
    assert_eq!(queued_timeline.len(), 2);
    assert!(queued_timeline.iter().any(|(iid, _)| iid == &id_a));
    assert!(queued_timeline.iter().any(|(iid, _)| iid == &id_c));
    assert!(!queued_timeline.iter().any(|(iid, _)| iid == &id_b));
}

#[test]
fn test_conversation_keeps_tool_args_preview() {
    let mut model = ConversationModel::default();
    model.apply(StartChat {
        submission: "read file".to_string(),
    });
    model.apply(ObserveToolCallStart {
        chat_id: super::ids::ChatId::new("chat-1"),
        turn_id: super::ids::ChatTurnId::new("turn-1"),
        id: super::ids::ToolCallId::new("tool-1"),
        provider_id: None,
        name: "Read".to_string(),
        index: 0,
    });
    model.apply(ObserveToolCallUpdate {
        chat_id: super::ids::ChatId::new("chat-1"),
        turn_id: super::ids::ChatTurnId::new("turn-1"),
        id: super::ids::ToolCallId::new("tool-1"),
        provider_id: None,
        name: "Read".to_string(),
        index: 0,
        arguments: Some(r#"{"file_path":"src/main.rs"}"#.to_string()),
        status: ToolCallStatus::Ready,
    });
    model.apply(ObserveToolCallUpdate {
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
    model.apply(ObserveToolCallUpdate {
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
    model.apply(ObserveAssistantText {
        chat_id: super::ids::ChatId::new("chat-1"),
        turn_id: super::ids::ChatTurnId::new("turn-1"),
        text: "让我来审查".to_string(),
    });
    model.apply(CompleteBlock {
        chat_id: super::ids::ChatId::new("chat-1"),
        turn_id: super::ids::ChatTurnId::new("turn-1"),
    });
    // ToolCallStart 用纯 tool 序号 index=0
    model.apply(ObserveToolCallStart {
        chat_id: super::ids::ChatId::new("chat-1"),
        turn_id: super::ids::ChatTurnId::new("turn-1"),
        id: super::ids::ToolCallId::new("tool-1"),
        provider_id: None,
        name: "Agent".to_string(),
        index: 0,
    });
    // ToolCall 用 content_block index=1（因为 text 占了 block 0）
    model.apply(ObserveToolCallUpdate {
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
    let changes = model.apply(ObserveToolResult {
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
    model.apply(ObserveAssistantText {
        chat_id: super::ids::ChatId::new("chat-1"),
        turn_id: super::ids::ChatTurnId::new("turn-1"),
        text: "让我".to_string(),
    });
    // 不调 CompleteBlock — text 还在 streaming
    model.apply(ObserveToolCallStart {
        chat_id: super::ids::ChatId::new("chat-1"),
        turn_id: super::ids::ChatTurnId::new("turn-1"),
        id: super::ids::ToolCallId::new("tool-1"),
        provider_id: None,
        name: "Agent".to_string(),
        index: 0,
    });
    model.apply(ObserveToolCallUpdate {
        chat_id: super::ids::ChatId::new("chat-1"),
        turn_id: super::ids::ChatTurnId::new("turn-1"),
        provider_id: Some("provider-1".to_string()),
        id: super::ids::ToolCallId::new("call_abc"),
        name: "Agent".to_string(),
        index: 1,
        arguments: None,
        status: ToolCallStatus::Ready,
    });
    let changes = model.apply(ObserveToolResult {
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
    model.apply(ObserveToolCallUpdate {
        chat_id: super::ids::ChatId::new("chat-1"),
        turn_id: super::ids::ChatTurnId::new("turn-1"),
        provider_id: Some("provider-1".to_string()),
        id: super::ids::ToolCallId::new("call_agent_no_start"),
        name: "Agent".to_string(),
        index: 0,
        arguments: None,
        status: ToolCallStatus::Ready,
    });
    let changes = model.apply(ObserveToolResult {
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

/// timeline 镜像验证：完整回合（user / assistant / tool-call / tool-result）后
/// timeline 应包含 UserMessage、AssistantText、ToolCall、ToolResult，
/// 且 AgentProgress **不进 timeline**（进度通过 tool_calls[].activities 内联渲染）。
#[test]
fn test_timeline_mirrors_blocks_no_agent_progress() {
    let mut model = ConversationModel::default();
    let chat_id = super::ids::ChatId::new("chat-a42");
    let turn_id = super::ids::ChatTurnId::new("turn-a42");
    let tool_id = super::ids::ToolCallId::new("tool-a42");

    // 1. 用户消息
    model.apply(StartChat {
        submission: "run task".to_string(),
    });

    // 2. Assistant text
    model.apply(ObserveAssistantText {
        chat_id: chat_id.clone(),
        turn_id: turn_id.clone(),
        text: "starting agent".to_string(),
    });
    model.apply(CompleteBlock {
        chat_id: chat_id.clone(),
        turn_id: turn_id.clone(),
    });

    // 3. Tool call start
    model.apply(ObserveToolCallStart {
        chat_id: chat_id.clone(),
        turn_id: turn_id.clone(),
        id: tool_id.clone(),
        provider_id: None,
        name: "Agent".to_string(),
        index: 0,
    });

    // 4. Agent progress — 不进 timeline，只写入 tool_calls[].activities
    model.apply(RecordAgentProgress {
        chat_id: chat_id.clone(),
        turn_id: turn_id.clone(),
        tool_id: tool_id.clone(),
        message: "analysing codebase".to_string(),
    });

    // 5. Tool result
    model.apply(ObserveToolResult {
        chat_id: chat_id.clone(),
        turn_id: turn_id.clone(),
        id: tool_id.clone(),
        provider_id: "provider-a42".to_string(),
        tool_name: "Agent".to_string(),
        output: "done".to_string(),
        content: serde_json::json!({ "text": "done" }),
        is_error: false,
        image_count: 0,
    });

    // 断言 AgentProgress 不在 timeline（防双显示）
    let has_agent_progress = model
        .timeline
        .items()
        .iter()
        .any(|item| matches!(item, OutputTimelineItem::AgentProgress { .. }));
    assert!(
        !has_agent_progress,
        "timeline.items() MUST NOT contain AgentProgress (it is inline-rendered via \
         tool_calls[].activities); items = {:?}",
        model
            .timeline
            .items()
            .iter()
            .map(|i| i.id().into_owned())
            .collect::<Vec<_>>()
    );

    // 进度消息写入对应 tool_call.activities（内联渲染路径）
    let turn = model
        .chats
        .iter()
        .flat_map(|ch| ch.turns.iter())
        .find(|t| t.id == turn_id);
    let activities = turn
        .and_then(|t| {
            t.tool_calls.iter().find(|c| {
                c.id.as_ref()
                    .is_some_and(|id| id.as_ref() == tool_id.to_string())
            })
        })
        .map(|c| c.activities.clone())
        .unwrap_or_default();
    assert!(
        activities.iter().any(|a| a.contains("analysing codebase")),
        "tool_call.activities should contain the progress message; activities = {activities:?}"
    );

    // 全 timeline 条目的 id 不重复（种类完整、无重）
    let ids: Vec<_> = model
        .timeline
        .items()
        .iter()
        .map(|i| i.id().into_owned())
        .collect();
    let unique_count = ids.iter().collect::<std::collections::HashSet<_>>().len();
    assert_eq!(
        ids.len(),
        unique_count,
        "timeline ids should be unique; ids = {ids:?}"
    );
}

// --- A4.3 基线测试：位置查询改读 timeline ---

/// A4.3 TDD 基线：insert_tool_call_block_before_active_text 用 timeline 去重
/// 同一 (chat, turn, id) 的 ToolCall 重复触发后，timeline 中只有 1 个 ToolCall ref。
#[test]
fn test_a43_insert_tool_call_dedup_reads_timeline() {
    let mut model = ConversationModel::default();
    let chat_id = super::ids::ChatId::new("chat-a43-dedup");
    let turn_id = super::ids::ChatTurnId::new("turn-a43-dedup");
    let tool_id = super::ids::ToolCallId::new("tool-a43-dedup");

    model.ensure_runtime_turn(chat_id.clone(), turn_id.clone());

    // ToolCallStart → 第 1 次插入
    model.apply(ObserveToolCallStart {
        chat_id: chat_id.clone(),
        turn_id: turn_id.clone(),
        id: tool_id.clone(),
        provider_id: None,
        name: "Read".to_string(),
        index: 0,
    });

    // ToolCallUpdate with same id → 不应重复插入 ToolCall
    model.apply(ObserveToolCallUpdate {
        chat_id: chat_id.clone(),
        turn_id: turn_id.clone(),
        id: tool_id.clone(),
        provider_id: None,
        name: "Read".to_string(),
        index: 0,
        arguments: Some(r#"{"file_path":"main.rs"}"#.to_string()),
        status: ToolCallStatus::Ready,
    });

    let tool_call_count = model
        .timeline
        .items()
        .iter()
        .filter(|item| {
            matches!(
                item,
                OutputTimelineItem::ToolCall { reference }
                    if reference.context.chat_id == chat_id
                        && reference.context.turn_id == turn_id
                        && reference.tool_call_id == tool_id
            )
        })
        .count();

    assert_eq!(
        tool_call_count, 1,
        "timeline 中相同 (chat, turn, id) 的 ToolCall 应仅出现 1 次（去重）"
    );
}

/// A4.3 TDD 基线：promote_orphan_tool_result 后 timeline 中孤儿消失、ToolResult 出现
/// 且 ToolResult 排在 ToolCall 之后（顺序等价）。
#[test]
fn test_a43_promote_orphan_timeline_ordering() {
    let mut model = ConversationModel::default();
    let chat_id = super::ids::ChatId::new("chat-a43-orphan");
    let turn_id = super::ids::ChatTurnId::new("turn-a43-orphan");
    let tool_id = super::ids::ToolCallId::new("tool-a43-orphan");

    model.ensure_runtime_turn(chat_id.clone(), turn_id.clone());

    // 先到达 ToolResult（孤儿）
    let changes = model.apply(ObserveToolResult {
        chat_id: chat_id.clone(),
        turn_id: turn_id.clone(),
        provider_id: "prov-a43".to_string(),
        id: tool_id.clone(),
        tool_name: "Bash".to_string(),
        output: "output-a43".to_string(),
        content: serde_json::json!({}),
        is_error: false,
        image_count: 0,
    });
    assert!(
        changes
            .iter()
            .any(|c| matches!(c, ConversationChange::OrphanToolResultObserved { .. })),
        "先到达的 ToolResult 应为孤儿"
    );

    // 验证孤儿在 timeline 中
    let has_orphan = model.timeline.items().iter().any(|item| {
        matches!(item, OutputTimelineItem::OrphanToolResult { id, .. } if id == tool_id.as_ref())
    });
    assert!(has_orphan, "孤儿应出现在 timeline 中");

    // ToolCallStart → 触发 promote_orphan_tool_result
    model.apply(ObserveToolCallStart {
        chat_id: chat_id.clone(),
        turn_id: turn_id.clone(),
        id: tool_id.clone(),
        provider_id: None,
        name: "Bash".to_string(),
        index: 0,
    });

    // ToolCallUpdate → 触发 promote_orphan_tool_result（confirm binding）
    model.apply(ObserveToolCallUpdate {
        chat_id: chat_id.clone(),
        turn_id: turn_id.clone(),
        id: tool_id.clone(),
        provider_id: Some("prov-a43".to_string()),
        name: "Bash".to_string(),
        index: 0,
        arguments: None,
        status: ToolCallStatus::Ready,
    });

    // 孤儿应从 timeline 中消失
    let still_orphan = model.timeline.items().iter().any(|item| {
        matches!(item, OutputTimelineItem::OrphanToolResult { id, .. } if id == tool_id.as_ref())
    });
    assert!(!still_orphan, "孤儿提升后应从 timeline 移除");

    // ToolCall 和 ToolResult 均应存在于 timeline，且顺序正确
    let positions: Vec<_> = model
        .timeline
        .items()
        .iter()
        .enumerate()
        .filter_map(|(idx, item)| match item {
            OutputTimelineItem::ToolCall { reference }
            | OutputTimelineItem::ToolResult { reference }
                if reference.tool_call_id == tool_id =>
            {
                Some(idx)
            }
            _ => None,
        })
        .collect();

    assert_eq!(positions.len(), 2, "应有 ToolCall + ToolResult 各一个");
    assert!(
        positions[0] < positions[1],
        "孤儿提升后 ToolResult 应排在 ToolCall 之后（顺序等价）"
    );
}

/// A4.3 TDD 基线：observe_tool_call_update 中已存在的 ToolCall 不重复插入 timeline。
/// 复现 model.rs:313 的查询点：existing_tool_position 改读 timeline。
#[test]
fn test_a43_observe_tool_call_update_no_duplicate_timeline_entry() {
    let mut model = ConversationModel::default();
    let chat_id = super::ids::ChatId::new("chat-a43-dup");
    let turn_id = super::ids::ChatTurnId::new("turn-a43-dup");
    let tool_id = super::ids::ToolCallId::new("tool-a43-dup");

    model.ensure_runtime_turn(chat_id.clone(), turn_id.clone());

    // First: ToolCallStart inserts the tool
    model.apply(ObserveToolCallStart {
        chat_id: chat_id.clone(),
        turn_id: turn_id.clone(),
        id: tool_id.clone(),
        provider_id: None,
        name: "Write".to_string(),
        index: 0,
    });

    // Multiple ToolCallUpdates with same id — should NOT create duplicate ToolCall entries
    for _ in 0..3 {
        model.apply(ObserveToolCallUpdate {
            chat_id: chat_id.clone(),
            turn_id: turn_id.clone(),
            id: tool_id.clone(),
            provider_id: Some("prov-a43-dup".to_string()),
            name: "Write".to_string(),
            index: 0,
            arguments: Some(r#"{"file_path":"a.txt"}"#.to_string()),
            status: ToolCallStatus::Ready,
        });
    }

    let count = model
        .timeline
        .items()
        .iter()
        .filter(|item| {
            matches!(
                item,
                OutputTimelineItem::ToolCall { reference }
                    if reference.context.chat_id == chat_id
                       && reference.context.turn_id == turn_id
                       && reference.tool_call_id == tool_id
            )
        })
        .count();

    assert_eq!(
        count, 1,
        "多次 ToolCallUpdate 后 timeline 中该 ToolCall 仍只有 1 项"
    );
}
