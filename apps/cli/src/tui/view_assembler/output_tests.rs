use super::OutputViewAssembler;
use crate::tui::model::conversation::intent::ConversationIntent;
use crate::tui::model::conversation::model::ConversationModel;
use crate::tui::view_model::{OutputBlockKind, ToolSemanticStatus};

#[test]
fn test_output_assembler_maps_tool_status_to_icon() {
    let mut conversation = ConversationModel::default();
    add_completed_tool_after_thinking(&mut conversation, "Read", "ok");

    let vm = OutputViewAssembler::assemble_from_conversation(&conversation, 7);
    let tool = vm
        .blocks
        .iter()
        .find_map(|block| match &block.kind {
            OutputBlockKind::ToolCall(tool) => Some(tool),
            _ => None,
        })
        .expect("tool block");

    assert_eq!(tool.icon, "✓");
    assert_eq!(tool.semantic_status, ToolSemanticStatus::Success);
}

#[test]
fn test_output_assembler_keeps_tool_result_inside_tool_after_thinking() {
    let mut conversation = ConversationModel::default();
    add_completed_tool_after_thinking(
        &mut conversation,
        "Grep",
        "/tmp/docs/bug/active.md:18:match",
    );

    let vm = OutputViewAssembler::assemble_from_conversation(&conversation, 7);
    let diagnostic_results = vm
        .blocks
        .iter()
        .filter(|block| matches!(&block.kind, OutputBlockKind::DiagnosticNotice(_)))
        .count();
    let tool = vm
        .blocks
        .iter()
        .find_map(|block| match &block.kind {
            OutputBlockKind::ToolCall(tool) => Some(tool),
            _ => None,
        })
        .expect("tool block");

    assert_eq!(diagnostic_results, 0);
    assert_eq!(tool.title, "Grep");
    assert_eq!(tool.result_summary.as_deref(), Some("✓ Grep completed"));
}

#[test]
fn test_output_assembler_summarizes_embedded_tool_result_without_full_output() {
    let mut conversation = ConversationModel::default();
    let full_output = "line1\nline2\nline3\nline4\nline5\nline6";
    add_completed_tool_after_thinking(&mut conversation, "Read", full_output);

    let vm = OutputViewAssembler::assemble_from_conversation(&conversation, 7);
    let diagnostic_results = vm
        .blocks
        .iter()
        .filter(|block| matches!(&block.kind, OutputBlockKind::DiagnosticNotice(_)))
        .count();
    let tool = vm
        .blocks
        .iter()
        .find_map(|block| match &block.kind {
            OutputBlockKind::ToolCall(tool) => Some(tool),
            _ => None,
        })
        .expect("tool block");

    assert_eq!(diagnostic_results, 0);
    assert_eq!(tool.result_summary.as_deref(), Some("✓ Read completed"));
    assert!(!tool
        .result_summary
        .as_deref()
        .unwrap_or_default()
        .contains("line1"));
}

#[test]
fn test_output_assembler_uses_error_summary_for_failed_tool_result() {
    let mut conversation = ConversationModel::default();
    add_failed_tool_after_thinking(&mut conversation, "Read", "permission denied");

    let vm = OutputViewAssembler::assemble_from_conversation(&conversation, 7);
    let tool = vm
        .blocks
        .iter()
        .find_map(|block| match &block.kind {
            OutputBlockKind::ToolCall(tool) => Some(tool),
            _ => None,
        })
        .expect("tool block");

    assert_eq!(tool.result_summary.as_deref(), Some("✗ Read failed"));
}

fn add_failed_tool_after_thinking(conversation: &mut ConversationModel, name: &str, output: &str) {
    add_tool_after_thinking(conversation, name, output, true);
}

fn add_completed_tool_after_thinking(
    conversation: &mut ConversationModel,
    name: &str,
    output: &str,
) {
    add_tool_after_thinking(conversation, name, output, false);
}

fn add_tool_after_thinking(
    conversation: &mut ConversationModel,
    name: &str,
    output: &str,
    is_error: bool,
) {
    conversation.apply(ConversationIntent::StartChat {
        submission: "search".to_string(),
    });
    conversation.apply(ConversationIntent::ObserveThinkingText {
        text: "thinking".to_string(),
    });
    conversation.apply(ConversationIntent::CompleteTextBlock);
    conversation.apply(ConversationIntent::ObserveToolCallStart {
        name: name.to_string(),
        index: 0,
    });
    conversation.apply(ConversationIntent::ObserveToolCall {
        id: "tool-1".to_string(),
        name: name.to_string(),
        index: 0,
        summary: "search docs".to_string(),
    });
    conversation.apply(ConversationIntent::ObserveToolResult {
        id: "tool-1".to_string(),
        tool_name: name.to_string(),
        output: output.to_string(),
        is_error,
        image_count: 0,
    });
}
