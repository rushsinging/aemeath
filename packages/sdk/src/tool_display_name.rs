//! 工具内部名 → 用户可见友好名的**唯一真相源**（#1964 Task 3）。
//!
//! 下沉自 `apps/cli/src/tui/view_model/state.rs` 的 `TOOL_DISPLAY_NAMES`，
//! 供 TUI 渲染与 runtime（转后台摘要 `invocation_summary_text`）共同复用。
//!
//! - TUI 侧：`crate::tui::view_model::tool_name::tool_display_name` 委托到此处；
//! - runtime 侧：直接调用 `sdk::tool_display_name`。
//!
//! **NEVER** 在 runtime / cli 再抄一份字面量表（双源即债务）；新增工具名
//! 只允许改这一个 match。

/// 返回工具的用户可见 display name。未注册的工具原样返回内部名。
///
/// ```
/// assert_eq!(sdk::tool_display_name("Bash"), "Run");
/// assert_eq!(sdk::tool_display_name("Grep"), "Search");
/// assert_eq!(sdk::tool_display_name("NoSuchTool"), "NoSuchTool");
/// ```
pub fn tool_display_name(name: &str) -> &str {
    match name {
        "Bash" => "Run",
        "Glob" => "Find",
        "Grep" => "Search",
        "Read" => "Read",
        "Write" => "Write",
        "Edit" => "Edit",
        "Agent" => "Agent",
        "Skill" => "Skill",
        "WebFetch" => "Web Fetch",
        "WebSearch" => "Web Search",
        "ToolSearch" => "Tool Search",
        "EnterWorktree" => "Enter Worktree",
        "ExitWorktree" => "Exit Worktree",
        "AskUserQuestion" => "Ask",
        "TaskCreate" => "New Task",
        "TaskUpdate" => "Update Task",
        "TaskBlockBy" => "Block Task",
        "TaskGet" => "Task",
        "TaskListGet" => "Tasks",
        "TaskLists" => "Task Lists",
        "TaskListCreate" => "New Task List",
        "TaskListComplete" => "Complete List",
        "TaskStop" => "Stop Task",
        "BackgroundProcessList" => "Backend Processes",
        "BackgroundProcessStatus" => "Backend Process",
        "BackgroundProcessLogs" => "Backend Process Logs",
        "BackgroundProcessStop" => "Backend Process Stop",
        "BackgroundProcessWatch" => "Backend Process Watch",
        "MemoryAdd" => "Memory Add",
        "MemorySearch" => "Memory Search",
        "MemoryList" => "Memory List",
        "MemoryUpdate" => "Memory Update",
        "MemoryDelete" => "Memory Delete",
        other => other,
    }
}

#[cfg(test)]
#[path = "tool_display_name_tests.rs"]
mod tests;
