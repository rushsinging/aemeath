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
