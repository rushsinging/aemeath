//! 状态容器（#1146 placement 归位）。
use std::collections::HashMap;
use std::sync::LazyLock;

pub(crate) static TOOL_DISPLAY_NAMES: LazyLock<HashMap<&'static str, &'static str>> =
    LazyLock::new(|| {
        HashMap::from([
            ("Bash", "Run"),
            ("Glob", "Find"),
            ("Grep", "Search"),
            ("Read", "Read"),
            ("Write", "Write"),
            ("Edit", "Edit"),
            ("Agent", "Agent"),
            ("Skill", "Skill"),
            ("WebFetch", "Web Fetch"),
            ("WebSearch", "Web Search"),
            ("ToolSearch", "Tool Search"),
            ("EnterWorktree", "Enter Worktree"),
            ("ExitWorktree", "Exit Worktree"),
            ("AskUserQuestion", "Ask"),
            ("TaskCreate", "New Task"),
            ("TaskUpdate", "Update Task"),
            ("TaskBlockBy", "Block Task"),
            ("TaskGet", "Task"),
            ("TaskListGet", "Tasks"),
            ("TaskLists", "Task Lists"),
            ("BackgroundProcessList", "Backend Processes"),
            ("BackgroundProcessStatus", "Backend Process"),
            ("BackgroundProcessLogs", "Backend Process Logs"),
            ("BackgroundProcessStop", "Backend Process Stop"),
            ("BackgroundProcessWatch", "Backend Process Watch"),
            ("MemoryAdd", "Memory Add"),
            ("MemorySearch", "Memory Search"),
            ("MemoryList", "Memory List"),
            ("MemoryUpdate", "Memory Update"),
            ("MemoryDelete", "Memory Delete"),
            ("TaskListCreate", "New Task List"),
            ("TaskListComplete", "Complete List"),
            ("TaskStop", "Stop Task"),
        ])
    });
