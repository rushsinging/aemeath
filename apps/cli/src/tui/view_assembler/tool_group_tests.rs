use crate::tui::view_assembler::tool_group::{classify_tool_name, ToolGroupKind};

#[test]
fn classifies_explore_tools_with_stable_display_kind() {
    for tool_name in ["Read", "Glob", "Grep"] {
        assert_eq!(
            classify_tool_name(tool_name),
            Some(ToolGroupKind::Explore),
            "expected {tool_name} to belong to Explore",
        );
    }
}

#[test]
fn classifies_run_and_write_tools_without_cross_category_fallback() {
    assert_eq!(classify_tool_name("Bash"), Some(ToolGroupKind::Run));
    for tool_name in ["Write", "Edit"] {
        assert_eq!(
            classify_tool_name(tool_name),
            Some(ToolGroupKind::Write),
            "expected {tool_name} to belong to Write",
        );
    }
}

#[test]
fn classifies_only_the_explicit_task_allowlist() {
    for tool_name in [
        "TaskCreate",
        "TaskUpdate",
        "TaskBlockBy",
        "TaskListGet",
        "TaskLists",
        "TaskListCreate",
        "TaskListComplete",
        "TaskGet",
        "TaskStop",
    ] {
        assert_eq!(
            classify_tool_name(tool_name),
            Some(ToolGroupKind::Tasks),
            "expected {tool_name} to belong to Tasks",
        );
    }
}

#[test]
fn leaves_unknown_and_task_prefixed_tools_unclassified() {
    for tool_name in [
        "TaskList",
        "TaskFutureTool",
        "taskCreate",
        "mcp__custom_tool",
        "UnknownTool",
    ] {
        assert_eq!(
            classify_tool_name(tool_name),
            None,
            "expected {tool_name} to remain independently displayed",
        );
    }
}

#[test]
fn exposes_the_user_facing_explore_title() {
    assert_eq!(ToolGroupKind::Explore.title(), "Explore");
    assert_ne!(ToolGroupKind::Explore.title(), "Explor");
}
