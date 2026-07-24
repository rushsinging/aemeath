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

/// 为子代理 tool call 进度行提取简要预览文本。
///
/// 从 input JSON 中提取关键字段（如 command、file_path），
/// 截断到 80 字符。无匹配字段时回退到截断的 JSON 字符串。
/// 位于 view_model 层，adapter 可直接调用。
pub fn tool_input_preview(name: &str, input: &serde_json::Value) -> String {
    use serde_json::Value;

    let preview = match name {
        "Bash" => field_str(input, "command"),
        "Read" | "Write" | "Edit" => field_str(input, "file_path"),
        "Grep" => composite(input, &["pattern", "path"]),
        "Glob" => field_str(input, "pattern"),
        "WebFetch" => field_str(input, "url"),
        "WebSearch" => field_str(input, "query"),
        "EnterWorktree" | "ExitWorktree" => field_str(input, "path"),
        "TaskCreate" | "TaskUpdate" => field_str(input, "subject"),
        _ => {
            let raw = match input {
                Value::String(s) => s.clone(),
                value => value.to_string(),
            };
            raw.trim().to_string()
        }
    };

    truncate_preview(&preview)
}

fn field_str(input: &serde_json::Value, key: &str) -> String {
    input
        .get(key)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

fn composite(input: &serde_json::Value, keys: &[&str]) -> String {
    let parts: Vec<&str> = keys
        .iter()
        .filter_map(|k| input.get(k).and_then(|v| v.as_str()))
        .collect();
    parts.join(" ")
}

fn truncate_preview(s: &str) -> String {
    if s.is_empty() {
        return String::new();
    }
    let chars: String = s.chars().take(80).collect();
    if chars.len() < s.len() {
        format!("{chars}...")
    } else {
        chars
    }
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
    }

    #[test]
    fn test_unknown_returns_as_is() {
        assert_eq!(tool_display_name("SomeRandomTool"), "SomeRandomTool");
        assert_eq!(tool_display_name(""), "");
    }
}
