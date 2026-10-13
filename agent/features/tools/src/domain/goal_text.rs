//! 从工具 input 抽出一句话意图（#1964）。
//!
//! 规则：非空 `goal` 字段优先；否则按工具名拼主载荷（与当前 TUI header 同源语义）。

use serde_json::Value;

use super::constants::GOAL_TEXT_PREVIEW_CHARS;

/// 解析一次 tool call 的 goal 文本。
pub(crate) fn resolve(tool_name: &str, input: &Value) -> String {
    if let Some(goal) = non_empty_str(input, "goal") {
        return truncate(goal);
    }
    synthesize(tool_name, input)
}

fn synthesize(tool_name: &str, input: &Value) -> String {
    let text = match tool_name {
        "Bash" => owned_or_empty(non_empty_str(input, "command")),
        "Agent" => owned_or_empty(non_empty_str(input, "description")),
        "Glob" => owned_or_empty(non_empty_str(input, "pattern")),
        "Grep" => grep_summary(input),
        "Read" | "Write" | "Edit" => owned_or_empty(non_empty_str(input, "file_path")),
        "WebFetch" => owned_or_empty(non_empty_str(input, "url")),
        "WebSearch" | "ToolSearch" => owned_or_empty(non_empty_str(input, "query")),
        "Skill" => owned_or_empty(non_empty_str(input, "skill")),
        "AskUserQuestion" => owned_or_empty(non_empty_str(input, "question")),
        "MemoryAdd" => owned_or_empty(non_empty_str(input, "content")),
        "MemorySearch" => owned_or_empty(non_empty_str(input, "query")),
        "MemoryUpdate" | "MemoryDelete" => owned_or_empty(non_empty_str(input, "id")),
        "MemoryList" => "list".to_string(),
        "TaskCreate" => owned_or_empty(
            non_empty_str(input, "subject").or_else(|| non_empty_str(input, "description")),
        ),
        "TaskUpdate" | "TaskGet" | "TaskBlockBy" | "TaskStop" => {
            owned_or_empty(non_empty_str(input, "taskId").or_else(|| non_empty_str(input, "id")))
        }
        "TaskListCreate" => owned_or_empty(non_empty_str(input, "name")),
        "TaskListGet" | "TaskLists" | "TaskListComplete" => non_empty_str(input, "taskListId")
            .or_else(|| non_empty_str(input, "id"))
            .map(str::to_string)
            .unwrap_or_else(|| tool_name.to_string()),
        "BackgroundProcessStatus"
        | "BackgroundProcessLogs"
        | "BackgroundProcessStop"
        | "BackgroundProcessWatch" => owned_or_empty(non_empty_str(input, "task_id")),
        "BackgroundProcessList" => "list".to_string(),
        "EnterWorktree" => {
            owned_or_empty(non_empty_str(input, "branch").or_else(|| non_empty_str(input, "path")))
        }
        "ExitWorktree" => "exit".to_string(),
        _ => owned_or_empty(first_string_value(input)),
    };
    let trimmed = text.trim();
    if trimmed.is_empty() {
        tool_name.to_string()
    } else {
        truncate(trimmed)
    }
}

fn owned_or_empty(value: Option<&str>) -> String {
    value.unwrap_or("").to_string()
}

fn grep_summary(input: &Value) -> String {
    let pattern = non_empty_str(input, "pattern").unwrap_or("");
    let path = non_empty_str(input, "path").unwrap_or("");
    match (!pattern.is_empty(), !path.is_empty()) {
        (true, true) => format!("/{pattern}/, path={path}"),
        (true, false) => format!("/{pattern}/"),
        (false, true) => format!("path={path}"),
        (false, false) => String::new(),
    }
}

fn non_empty_str<'a>(input: &'a Value, key: &str) -> Option<&'a str> {
    input
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn first_string_value(input: &Value) -> Option<&str> {
    let object = input.as_object()?;
    object.values().find_map(|value| {
        value
            .as_str()
            .map(str::trim)
            .filter(|text| !text.is_empty())
    })
}

fn truncate(text: &str) -> String {
    let mut chars = text.chars();
    let prefix: String = chars.by_ref().take(GOAL_TEXT_PREVIEW_CHARS).collect();
    if chars.next().is_some() {
        format!("{prefix}…")
    } else {
        prefix
    }
}

#[cfg(test)]
#[path = "goal_text_tests.rs"]
mod tests;
