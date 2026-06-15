//! Tool display name 映射（内部名 → 用户可见名）。
//!
//! 位于 view_model 层，render 和 view_assembler 均可引用，
//! 避免 view_assembler 反向依赖 render 层。

use std::collections::HashMap;
use std::sync::LazyLock;

static TOOL_DISPLAY_NAMES: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    HashMap::from([
        ("Bash", "Run"),
        ("Glob", "Find"),
        ("Grep", "Search"),
        ("EnterWorktree", "Enter Worktree"),
        ("ExitWorktree", "Exit Worktree"),
        ("EnterPlanMode", "Enter Plan Mode"),
        ("ExitPlanMode", "Exit Plan Mode"),
        ("AskUserQuestion", "Ask"),
        ("TaskCreate", "Task"),
        ("TaskUpdate", "Task"),
        ("TaskGet", "Task"),
        ("TaskList", "Tasks"),
        ("TaskListCreate", "New Task List"),
        ("TaskListComplete", "Complete List"),
        ("TaskStop", "Stop Task"),
    ])
});

/// 返回工具的用户可见 display name。未注册的工具原样返回内部名。
pub fn tool_display_name(name: &str) -> &str {
    TOOL_DISPLAY_NAMES.get(name).copied().unwrap_or(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_known_mappings() {
        assert_eq!(tool_display_name("Bash"), "Run");
        assert_eq!(tool_display_name("Glob"), "Find");
        assert_eq!(tool_display_name("Grep"), "Search");
        assert_eq!(tool_display_name("EnterWorktree"), "Enter Worktree");
        assert_eq!(tool_display_name("ExitWorktree"), "Exit Worktree");
        assert_eq!(tool_display_name("EnterPlanMode"), "Enter Plan Mode");
        assert_eq!(tool_display_name("ExitPlanMode"), "Exit Plan Mode");
        assert_eq!(tool_display_name("AskUserQuestion"), "Ask");
        assert_eq!(tool_display_name("TaskCreate"), "Task");
        assert_eq!(tool_display_name("TaskUpdate"), "Task");
        assert_eq!(tool_display_name("TaskGet"), "Task");
        assert_eq!(tool_display_name("TaskList"), "Tasks");
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
        assert_eq!(tool_display_name("LSP"), "LSP");
    }

    #[test]
    fn test_unknown_returns_as_is() {
        assert_eq!(tool_display_name("SomeRandomTool"), "SomeRandomTool");
        assert_eq!(tool_display_name(""), "");
    }
}
