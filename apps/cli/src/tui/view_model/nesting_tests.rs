use super::{allowed_child, MAX_BLOCK_DEPTH};
use crate::tui::view_model::output::{
    OutputBlockKind, TextBlockView, ToolCallBlockView, ToolGroupBlockView, ToolGroupKind,
    ToolResultBlockView, ToolSemanticStatus,
};
use crate::tui::view_model::style::SemanticStyle;

fn tool_call() -> OutputBlockKind {
    OutputBlockKind::ToolCall(ToolCallBlockView {
        key: "call".into(),
        chat_id: None,
        run_id: None,
        tool_call_id: Some("call-1".into()),
        title: "Read".into(),
        icon: "●".into(),
        semantic_status: ToolSemanticStatus::Success,
        style: SemanticStyle::Success,
        args_preview: None,
        activity_lines: Vec::new(),
        result_summary: None,
        result_payload: None,
        workspace_root: None,
        collapsible: false,
        collapsed: false,
        agent_meta: None,
    })
}

fn tool_result() -> OutputBlockKind {
    OutputBlockKind::ToolResult(ToolResultBlockView {
        key: "result".into(),
        tool_title: "Read".into(),
        args_preview: None,
        result_text: "done".into(),
        data: None,
        style: SemanticStyle::Success,
    })
}

fn group() -> OutputBlockKind {
    OutputBlockKind::ToolGroup(ToolGroupBlockView {
        key: "group".into(),
        kind: ToolGroupKind::Explore,
        title: "Explore".into(),
        semantic_status: ToolSemanticStatus::Running,
        style: SemanticStyle::Running,
    })
}

fn text() -> OutputBlockKind {
    OutputBlockKind::AssistantMessage(TextBlockView {
        key: "text".into(),
        text: "reply".into(),
        style: SemanticStyle::Normal,
    })
}

#[test]
fn tool_group_accepts_tool_call_child() {
    assert!(allowed_child(&group(), &tool_call()));
}

#[test]
fn tool_group_rejects_non_tool_call_children_and_nested_groups() {
    assert!(!allowed_child(&group(), &tool_result()));
    assert!(!allowed_child(&group(), &text()));
    assert!(!allowed_child(&group(), &group()));
}

#[test]
fn tool_call_keeps_tool_result_child_contract() {
    assert!(allowed_child(&tool_call(), &tool_result()));
    assert!(!allowed_child(&tool_call(), &group()));
}

#[test]
fn nesting_depth_still_allows_group_call_result_tree() {
    assert_eq!(MAX_BLOCK_DEPTH, 4);
}
