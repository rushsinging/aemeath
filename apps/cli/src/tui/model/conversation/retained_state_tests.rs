use super::ids::{ChatId, ChatTurnId, ToolCallId};
use super::intent::{ConversationIntent, RecordAgentProgress, ToolCallStart};
use super::model::ConversationModel;

#[test]
fn retained_state_snapshot_separates_history_from_transient_state() {
    let mut model = ConversationModel::default();
    let chat_id = ChatId::new("chat-retained");
    let turn_id = ChatTurnId::new("turn-retained");
    let tool_id = ToolCallId::new("tool-retained");

    model.ensure_runtime_turn(chat_id.clone(), turn_id.clone());
    model.apply(ConversationIntent::ToolCallStart(ToolCallStart {
        chat_id: chat_id.clone(),
        turn_id: turn_id.clone(),
        id: tool_id.clone(),
        provider_id: None,
        name: "Agent".to_string(),
        index: 0,
    }));
    for index in 0..3 {
        model.apply(ConversationIntent::RecordAgentProgress(
            RecordAgentProgress {
                chat_id: chat_id.clone(),
                turn_id: turn_id.clone(),
                tool_id: tool_id.clone(),
                message: format!("progress-{index}"),
            },
        ));
    }
    let retained = model.retained_state_snapshot();
    assert_eq!(retained.chats, 1);
    assert_eq!(retained.turns, 1);
    assert_eq!(retained.tool_calls, 1);
    assert_eq!(retained.timeline_items, 1);
    assert_eq!(retained.agent_progress_entries, 3);
    assert_eq!(retained.agent_progress_bytes, 69);
    assert!(retained.output_view_journal_entries > 0);
    assert!(retained.output_view_journal_item_id_bytes > 0);
    assert!(!retained.has_active_interaction);
}

#[test]
fn retained_state_snapshot_returns_to_zero_after_reset() {
    let mut model = ConversationModel::default();
    model
        .agent_progress
        .push(super::agent_progress::AgentProgressEntry::new(
            "tool-1", "working",
        ));
    model.reset();

    let retained = model.retained_state_snapshot();
    assert_eq!(retained.chats, 0);
    assert_eq!(retained.turns, 0);
    assert_eq!(retained.tool_calls, 0);
    assert_eq!(retained.timeline_items, 0);
    assert_eq!(retained.agent_progress_entries, 0);
    assert_eq!(retained.agent_progress_bytes, 0);
    assert_eq!(retained.output_view_journal_entries, 1);
    assert_eq!(retained.output_view_journal_item_id_bytes, 0);
    assert!(!retained.has_active_interaction);
}
