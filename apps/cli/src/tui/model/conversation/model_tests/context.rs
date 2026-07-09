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
    model.apply(ToolCallStart {
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

