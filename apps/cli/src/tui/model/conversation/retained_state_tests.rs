use super::ids::{ChatId, ChatRunId, ToolCallId};
use super::intent::{ConversationIntent, RecordSubRunActivity, ToolCallStart};
use super::model::ConversationModel;
use crate::tui::adapter::tui_runtime_event::TuiSubRunActivityKind;

#[test]
fn sub_run_activity_retains_only_one_ordering_watermark_per_run() {
    let mut model = ConversationModel::default();
    let chat_id = ChatId::new("chat-retained");
    let run_id = ChatRunId::new("turn-retained");
    let tool_id = ToolCallId::new("tool-retained");
    model.ensure_runtime_turn(chat_id, run_id.clone());
    model.apply(ToolCallStart {
        chat_id: ChatId::new("chat-retained"),
        run_id: run_id.clone(),
        id: tool_id.clone(),
        provider_id: None,
        name: "Agent".to_string(),
        index: 0,
    });

    for sequence in 1..=100 {
        model.apply(RecordSubRunActivity {
            agent_id: "researcher".to_string(),
            sub_run_id: "sub-run".to_string(),
            parent_run_id: run_id.to_string(),
            spawned_by_tool_call_id: tool_id.clone(),
            sequence,
            sequence_index: 0,
            kind: TuiSubRunActivityKind::Text {
                text: format!("activity-{sequence}"),
            },
        });
    }

    let retained = model.retained_state_snapshot();
    assert_eq!(retained.sub_run_watermarks, 1);
    assert!(!retained.has_legacy_activity_history);
    assert_eq!(
        model
            .tool_call(&ChatId::new("chat-retained"), &run_id, &tool_id,)
            .expect("parent Agent ToolCall")
            .activities
            .len(),
        5,
        "presentation preview remains bounded independently of ordering state"
    );
}

#[test]
fn retained_state_snapshot_separates_history_from_transient_state() {
    let mut model = ConversationModel::default();
    let chat_id = ChatId::new("chat-retained");
    let run_id = ChatRunId::new("turn-retained");
    let tool_id = ToolCallId::new("tool-retained");

    model.ensure_runtime_turn(chat_id.clone(), run_id.clone());
    model.apply(ConversationIntent::ToolCallStart(ToolCallStart {
        chat_id: chat_id.clone(),
        run_id: run_id.clone(),
        id: tool_id.clone(),
        provider_id: None,
        name: "Agent".to_string(),
        index: 0,
    }));
    for index in 0..3 {
        model.record_agent_activities(
            chat_id.clone(),
            run_id.clone(),
            tool_id.clone(),
            vec![
                crate::tui::model::conversation::agent_activity::AgentActivityLine::message(
                    format!("progress-{index}"),
                ),
            ],
        );
    }
    let retained = model.retained_state_snapshot();
    assert_eq!(retained.chats, 1);
    assert_eq!(retained.runs, 1);
    assert_eq!(retained.tool_calls, 1);
    assert_eq!(retained.timeline_items, 1);
    assert_eq!(retained.sub_run_watermarks, 0);
    assert!(!retained.has_legacy_activity_history);
    assert!(retained.output_view_journal_entries > 0);
    assert!(retained.output_view_journal_item_id_bytes > 0);
    assert!(!retained.has_active_interaction);
}

#[test]
fn retained_state_snapshot_returns_to_zero_after_reset() {
    let mut model = ConversationModel::default();
    model
        .sub_run_watermarks
        .push(super::agent_activity::SubRunActivityWatermark {
            agent_id: "agent".to_string(),
            run_id: "sub-run".to_string(),
            sequence: 1,
            sequence_index: 0,
        });
    model.reset();

    let retained = model.retained_state_snapshot();
    assert_eq!(retained.chats, 0);
    assert_eq!(retained.runs, 0);
    assert_eq!(retained.tool_calls, 0);
    assert_eq!(retained.timeline_items, 0);
    assert_eq!(retained.sub_run_watermarks, 0);
    assert!(!retained.has_legacy_activity_history);
    assert_eq!(retained.output_view_journal_entries, 1);
    assert_eq!(retained.output_view_journal_item_id_bytes, 0);
    assert!(!retained.has_active_interaction);
}
