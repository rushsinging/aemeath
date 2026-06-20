use super::OutputViewAssembler;
use crate::tui::model::conversation::ids::ToolCallId;
use crate::tui::model::conversation::intent::ConversationIntent;
use crate::tui::model::conversation::model::ConversationModel;
use crate::tui::model::conversation::tool_call::ToolCallStatus;
use crate::tui::view_model::{OutputBlockKind, ToolSemanticStatus};

#[test]
fn test_output_assembler_renders_task_list_create_tool_call() {
    let mut conversation = ConversationModel::default();
    conversation.apply(ConversationIntent::StartChat {
        submission: "fix bug".to_string(),
    });
    add_task_tool(
        &mut conversation,
        "tool-tlc",
        "TaskListCreate",
        r#"{"subject":"修复 bug","summary":"修复 bug 84"}"#,
        "Task list #0 created",
    );

    let vm = OutputViewAssembler::assemble_from_conversation(&conversation, 7, None);
    let diagnostic_count = vm
        .roots
        .iter()
        .filter(|block| matches!(&block.kind, OutputBlockKind::DiagnosticNotice(_)))
        .count();
    assert_eq!(diagnostic_count, 0, "TaskListCreate 结果不应泄漏为诊断文本");

    let tool = vm
        .roots
        .iter()
        .find_map(|block| match &block.kind {
            OutputBlockKind::ToolCall(tool) => Some(tool),
            _ => None,
        })
        .expect("应有 TaskListCreate 工具调用块");

    assert_eq!(tool.title, "TaskListCreate");
    assert_eq!(tool.icon, "✓");
    assert_eq!(tool.semantic_status, ToolSemanticStatus::Success);
    assert!(tool.result_summary.is_some(), "应有结果摘要");
}

#[test]
fn test_output_assembler_renders_task_create_tool_call() {
    let mut conversation = ConversationModel::default();
    conversation.apply(ConversationIntent::StartChat {
        submission: "fix bug".to_string(),
    });
    add_task_tool(
        &mut conversation,
        "tool-tc",
        "TaskCreate",
        r#"{"subject":"分析代码","description":"查看代码结构"}"#,
        "Task #0 created",
    );

    let vm = OutputViewAssembler::assemble_from_conversation(&conversation, 7, None);
    let tool = vm
        .roots
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
    add_task_tool(
        &mut conversation,
        "tool-tu",
        "TaskUpdate",
        r#"{"taskId":"1","status":"completed"}"#,
        "Task #1 updated",
    );

    let vm = OutputViewAssembler::assemble_from_conversation(&conversation, 7, None);
    let tool = vm
        .roots
        .iter()
        .find_map(|block| match &block.kind {
            OutputBlockKind::ToolCall(tool) => Some(tool),
            _ => None,
        })
        .expect("应有 TaskUpdate 工具调用块");

    assert_eq!(tool.title, "TaskUpdate");
    assert_eq!(tool.icon, "✓");
}

fn add_task_tool(
    conversation: &mut ConversationModel,
    id: &str,
    name: &str,
    _summary: &str,
    output: &str,
) {
    conversation.apply(ConversationIntent::ObserveToolCallStart {
        chat_id: crate::tui::model::conversation::ids::ChatId::new("session-1"),
        turn_id: crate::tui::model::conversation::ids::ChatTurnId::new("turn-1"),
        id: ToolCallId::new(id),
        provider_id: None,
        name: name.to_string(),
        index: 0,
    });
    conversation.apply(ConversationIntent::ObserveToolCallUpdate {
        chat_id: crate::tui::model::conversation::ids::ChatId::new("session-1"),
        turn_id: crate::tui::model::conversation::ids::ChatTurnId::new("turn-1"),
        provider_id: Some(format!("provider-{id}")),
        id: ToolCallId::new(id),
        name: name.to_string(),
        index: 0,
        arguments: None,
        status: ToolCallStatus::Ready,
    });
    conversation.apply(ConversationIntent::ObserveToolResult {
        chat_id: crate::tui::model::conversation::ids::ChatId::new("session-1"),
        turn_id: crate::tui::model::conversation::ids::ChatTurnId::new("turn-1"),
        provider_id: format!("provider-{id}"),
        id: ToolCallId::new(id),
        tool_name: name.to_string(),
        output: output.to_string(),
        content: serde_json::json!({ "text": output }),
        is_error: false,
        image_count: 0,
    });
}
