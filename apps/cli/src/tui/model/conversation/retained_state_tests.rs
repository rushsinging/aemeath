use super::ids::{ChatId, ChatTurnId, ToolCallId};
use super::intent::{
    ConversationIntent, RecordAgentProgress, RunCompleted, RunStarted, RunStepCompleted,
    RunStepStarted, ToolCallStart,
};
use super::interaction::{UiRunId, UiRunStepId};
use super::model::ConversationModel;

#[test]
fn retained_state_snapshot_separates_history_from_transient_state() {
    let mut model = ConversationModel::default();
    let chat_id = ChatId::new("chat-retained");
    let turn_id = ChatTurnId::new("turn-retained");
    let tool_id = ToolCallId::new("tool-retained");
    let run_id = UiRunId::from("run-retained");

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
    model.apply(ConversationIntent::RunStarted(RunStarted {
        run_id: run_id.clone(),
    }));
    for index in 0..2 {
        let step_id = UiRunStepId::from(format!("step-{index}").as_str());
        model.apply(ConversationIntent::RunStepStarted(RunStepStarted {
            run_id: run_id.clone(),
            step_id: step_id.clone(),
            tool_reference: Some(tool_id.to_string()),
        }));
        model.apply(ConversationIntent::RunStepCompleted(RunStepCompleted {
            run_id: run_id.clone(),
            step_id,
        }));
    }
    model.apply(ConversationIntent::RunCompleted(RunCompleted {
        run_id: run_id.clone(),
    }));

    let retained = model.retained_state_snapshot();
    assert_eq!(retained.chats, 1);
    assert_eq!(retained.turns, 1);
    assert_eq!(retained.tool_calls, 1);
    assert_eq!(retained.timeline_items, 1);
    assert_eq!(retained.agent_progress_entries, 3);
    assert_eq!(retained.agent_progress_bytes, 69);
    assert_eq!(retained.agent_runs, 1);
    assert_eq!(retained.agent_run_steps, 2);
    assert_eq!(retained.terminal_agent_runs, 1);
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
    model.apply(ConversationIntent::RunStarted(RunStarted {
        run_id: UiRunId::from("run-1"),
    }));

    model.reset();

    let retained = model.retained_state_snapshot();
    assert_eq!(retained.chats, 0);
    assert_eq!(retained.turns, 0);
    assert_eq!(retained.tool_calls, 0);
    assert_eq!(retained.timeline_items, 0);
    assert_eq!(retained.agent_progress_entries, 0);
    assert_eq!(retained.agent_progress_bytes, 0);
    assert_eq!(retained.agent_runs, 0);
    assert_eq!(retained.agent_run_steps, 0);
    assert_eq!(retained.terminal_agent_runs, 0);
    assert_eq!(retained.output_view_journal_entries, 1);
    assert_eq!(retained.output_view_journal_item_id_bytes, 0);
    assert!(!retained.has_active_interaction);
}
