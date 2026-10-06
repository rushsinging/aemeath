use super::*;

#[test]
fn test_known_mappings() {
    assert_eq!(tool_display_name("Bash"), "Run");
    assert_eq!(tool_display_name("Glob"), "Find");
    assert_eq!(tool_display_name("Grep"), "Search");
    assert_eq!(tool_display_name("EnterWorktree"), "Enter Worktree");
    assert_eq!(tool_display_name("ExitWorktree"), "Exit Worktree");
    assert_eq!(tool_display_name("AskUserQuestion"), "Ask");
    assert_eq!(tool_display_name("TaskCreate"), "New Task");
    assert_eq!(tool_display_name("TaskUpdate"), "Update Task");
    assert_eq!(tool_display_name("TaskBlockBy"), "Block Task");
    assert_eq!(tool_display_name("TaskGet"), "Task");
    assert_eq!(tool_display_name("TaskListGet"), "Tasks");
    assert_eq!(tool_display_name("TaskList"), "Tasks");
    assert_eq!(tool_display_name("TaskLists"), "Task Lists");
    assert_eq!(tool_display_name("TaskListCreate"), "New Task List");
    assert_eq!(tool_display_name("TaskListComplete"), "Complete List");
    assert_eq!(tool_display_name("TaskStop"), "Stop Task");
}

#[test]
fn test_unmapped_returns_internal_name() {
    assert_eq!(tool_display_name("Read"), "Read");
    assert_eq!(tool_display_name("Write"), "Write");
    assert_eq!(tool_display_name("Edit"), "Edit");
    assert_eq!(tool_display_name("Agent"), "Agent");
    assert_eq!(tool_display_name("WebFetch"), "WebFetch");
    assert_eq!(tool_display_name("Skill"), "Skill");
}

#[test]
fn test_unknown_returns_as_is() {
    assert_eq!(tool_display_name("SomeRandomTool"), "SomeRandomTool");
    assert_eq!(tool_display_name(""), "");
}
