//! 状态容器（#1146 placement 归位）。
use std::collections::HashMap;
use std::sync::LazyLock;

pub(crate) static TOOL_DISPLAY_NAMES: LazyLock<HashMap<&'static str, &'static str>> =
    LazyLock::new(|| {
        HashMap::from([
            ("Bash", "Run"),
            ("Glob", "Find"),
            ("Grep", "Search"),
            ("EnterWorktree", "Enter Worktree"),
            ("ExitWorktree", "Exit Worktree"),
            ("AskUserQuestion", "Ask"),
            ("TaskCreate", "New Task"),
            ("TaskUpdate", "Update Task"),
            ("TaskBlockBy", "Block Task"),
            ("TaskGet", "Task"),
            ("TaskListGet", "Tasks"),
            ("TaskList", "Tasks"),
            ("TaskLists", "Task Lists"),
            ("TaskListCreate", "New Task List"),
            ("TaskListComplete", "Complete List"),
            ("TaskStop", "Stop Task"),
        ])
    });
