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
fn test_output_assembler_late_bound_tool_result_stays_inside_tool_block() {
    let mut conversation = ConversationModel::default();
    conversation.apply(ConversationIntent::StartChat {
        submission: "edit docs".to_string(),
    });
    conversation.apply(ConversationIntent::ObserveToolCallStart {
        name: "Edit".to_string(),
        index: 0,
    });
    conversation.apply(ConversationIntent::ObserveToolResult {
        id: "tool-1".to_string(),
        tool_name: "Edit".to_string(),
        output: "replaced 1 occurrence(s) in docs/bug/active.md\n---DIFF---\nold\n---DIFF---\nnew"
            .to_string(),
        is_error: false,
        image_count: 0,
    });
    conversation.apply(ConversationIntent::ObserveToolCall {
        id: "tool-1".to_string(),
        name: "Edit".to_string(),
        index: 0,
        summary: r#"{"file_path":"docs/bug/active.md"}"#.to_string(),
    });

    let vm = OutputViewAssembler::assemble_from_conversation(&conversation, 7);
    let diagnostics = vm
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

    assert_eq!(diagnostics, 0, "已绑定工具结果不应泄漏成块外诊断文本");
    assert_eq!(tool.title, "Edit");
    assert!(tool
        .result_summary
        .as_deref()
        .unwrap_or_default()
        .contains("Edit completed"));
    assert!(!tool
        .result_summary
        .as_deref()
        .unwrap_or_default()
        .contains("---DIFF---"));
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

#[test]
fn test_output_assembler_renders_task_list_create_tool_call() {
    let mut conversation = ConversationModel::default();
    conversation.apply(ConversationIntent::StartChat {
        submission: "fix bug".to_string(),
    });
    conversation.apply(ConversationIntent::ObserveToolCallStart {
        name: "TaskListCreate".to_string(),
        index: 0,
    });
    conversation.apply(ConversationIntent::ObserveToolCall {
        id: "tool-tlc".to_string(),
        name: "TaskListCreate".to_string(),
        index: 0,
        summary: r#"{"subject":"修复 bug","summary":"修复 bug 84"}"#.to_string(),
    });
    conversation.apply(ConversationIntent::ObserveToolResult {
        id: "tool-tlc".to_string(),
        tool_name: "TaskListCreate".to_string(),
        output: "Task list #0 created".to_string(),
        is_error: false,
        image_count: 0,
    });

    let vm = OutputViewAssembler::assemble_from_conversation(&conversation, 7);

    // 应有且仅有一个 ToolCall block（不应泄漏为 DiagnosticNotice）
    let diagnostic_count = vm
        .blocks
        .iter()
        .filter(|block| matches!(&block.kind, OutputBlockKind::DiagnosticNotice(_)))
        .count();
    assert_eq!(diagnostic_count, 0, "TaskListCreate 结果不应泄漏为诊断文本");

    let tool = vm
        .blocks
        .iter()
        .find_map(|block| match &block.kind {
            OutputBlockKind::ToolCall(tool) => Some(tool),
            _ => None,
        })
        .expect("应有 TaskListCreate 工具调用块");

    assert_eq!(tool.title, "TaskListCreate");
    assert_eq!(tool.icon, "✓");
    assert_eq!(tool.semantic_status, ToolSemanticStatus::Success);
    // result_summary 应使用 fallback（TaskListCreateDisplay 返回空，走 default）
    assert!(tool.result_summary.is_some(), "应有结果摘要");
}

#[test]
fn test_output_assembler_renders_task_create_tool_call() {
    let mut conversation = ConversationModel::default();
    conversation.apply(ConversationIntent::StartChat {
        submission: "fix bug".to_string(),
    });
    conversation.apply(ConversationIntent::ObserveToolCallStart {
        name: "TaskCreate".to_string(),
        index: 0,
    });
    conversation.apply(ConversationIntent::ObserveToolCall {
        id: "tool-tc".to_string(),
        name: "TaskCreate".to_string(),
        index: 0,
        summary: r#"{"subject":"分析代码","description":"查看代码结构"}"#.to_string(),
    });
    conversation.apply(ConversationIntent::ObserveToolResult {
        id: "tool-tc".to_string(),
        tool_name: "TaskCreate".to_string(),
        output: "Task #0 created".to_string(),
        is_error: false,
        image_count: 0,
    });

    let vm = OutputViewAssembler::assemble_from_conversation(&conversation, 7);
    let tool = vm
        .blocks
        .iter()
        .find_map(|block| match &block.kind {
            OutputBlockKind::ToolCall(tool) => Some(tool),
            _ => None,
        })
        .expect("应有 TaskCreate 工具调用块");

    assert_eq!(tool.title, "TaskCreate");
    assert_eq!(tool.icon, "✓");
    assert_eq!(tool.semantic_status, ToolSemanticStatus::Success);
}

#[test]
fn test_output_assembler_renders_task_update_tool_call() {
    let mut conversation = ConversationModel::default();
    conversation.apply(ConversationIntent::StartChat {
        submission: "fix bug".to_string(),
    });
    conversation.apply(ConversationIntent::ObserveToolCallStart {
        name: "TaskUpdate".to_string(),
        index: 0,
    });
    conversation.apply(ConversationIntent::ObserveToolCall {
        id: "tool-tu".to_string(),
        name: "TaskUpdate".to_string(),
        index: 0,
        summary: r#"{"taskId":"1","status":"completed"}"#.to_string(),
    });
    conversation.apply(ConversationIntent::ObserveToolResult {
        id: "tool-tu".to_string(),
        tool_name: "TaskUpdate".to_string(),
        output: "Task #1 updated".to_string(),
        is_error: false,
        image_count: 0,
    });

    let vm = OutputViewAssembler::assemble_from_conversation(&conversation, 7);
    let tool = vm
        .blocks
        .iter()
        .find_map(|block| match &block.kind {
            OutputBlockKind::ToolCall(tool) => Some(tool),
            _ => None,
        })
        .expect("应有 TaskUpdate 工具调用块");

    assert_eq!(tool.title, "TaskUpdate");
    assert_eq!(tool.icon, "✓");
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
