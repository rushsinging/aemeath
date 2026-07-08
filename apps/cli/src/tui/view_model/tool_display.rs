use super::conversation::tool_result_payload::ToolResultPayload;
use super::tool_name::tool_display_name;
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::path::Path;

const HEADER_TRUNCATE: usize = 80;
const DETAIL_TRUNCATE: usize = 200;
const PATH_TRUNCATE: usize = 60;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolHeaderView {
    pub title: String,
    pub details: Vec<String>,
}

impl ToolHeaderView {
    pub fn new(title: impl Into<String>, details: Vec<String>) -> Self {
        Self {
            title: title.into(),
            details,
        }
    }
}

pub fn format_tool_header_view(
    name: &str,
    input: &Value,
    result_payload: Option<&ToolResultPayload>,
    workspace_root: Option<&Path>,
) -> ToolHeaderView {
    match name {
        "Bash" => bash_header(input, result_payload),
        "Read" => read_header(input, result_payload, workspace_root),
        "Write" => write_header(input, result_payload, workspace_root),
        "Edit" => edit_header(input, result_payload, workspace_root),
        "Glob" => glob_header(input, result_payload),
        "Grep" => grep_header(input, result_payload, workspace_root),
        "Agent" => agent_header(input, result_payload),
        "EnterWorktree" => enter_worktree_header(input, result_payload),
        "ExitWorktree" => exit_worktree_header(result_payload),
        "WebFetch" => web_fetch_header(input, result_payload),
        "AskUserQuestion" => ask_user_header(result_payload),
        _ => fallback_header(name, input),
    }
}

fn bash_header(input: &Value, result_payload: Option<&ToolResultPayload>) -> ToolHeaderView {
    let cmd = str_arg(input, "command", "");
    let suffix = typed_data::<sdk::tool_result::BashResult>(result_payload)
        .and_then(|result| {
            if result.exit_code == 0 {
                None
            } else if let Some(signal) = result.signal {
                Some(format!(" (signal {signal})"))
            } else {
                Some(format!(" (exit {})", result.exit_code))
            }
        })
        .unwrap_or_default();
    let title = if cmd.is_empty() {
        format!("{}{}", tool_display_name("Bash"), suffix)
    } else {
        format!(
            "{} {}{}",
            tool_display_name("Bash"),
            truncate_ellipsis(cmd, HEADER_TRUNCATE),
            suffix
        )
    };
    let details = if cmd.is_empty() {
        vec![]
    } else {
        vec![truncate_ellipsis(cmd, DETAIL_TRUNCATE)]
    };
    ToolHeaderView::new(title, details)
}

fn read_header(
    input: &Value,
    result_payload: Option<&ToolResultPayload>,
    workspace_root: Option<&Path>,
) -> ToolHeaderView {
    let path = file_path(input);
    let rel = display_path(path, workspace_root);
    let display_path = truncate_ellipsis_tail(&rel, PATH_TRUNCATE);
    let actual_lines = typed_data::<sdk::tool_result::ReadResult>(result_payload)
        .map(|result| result.line_count as usize)
        .or_else(|| {
            result_payload.and_then(|payload| parse_line_count_from_message(&payload.output))
        });
    let range = read_range_info(input, actual_lines);
    let title = if range.is_empty() {
        format!("{} {display_path}", tool_display_name("Read"))
    } else {
        format!("{} {display_path} {range}", tool_display_name("Read"))
    };
    ToolHeaderView::new(title, vec![])
}

fn write_header(
    input: &Value,
    result_payload: Option<&ToolResultPayload>,
    workspace_root: Option<&Path>,
) -> ToolHeaderView {
    let path = file_path(input);
    let rel = display_path(path, workspace_root);
    let display_path = truncate_ellipsis_tail(&rel, PATH_TRUNCATE);
    let actual_bytes = typed_data::<sdk::tool_result::WriteResult>(result_payload)
        .map(|result| result.bytes_written as usize)
        .or_else(|| result_payload.and_then(|payload| parse_bytes_from_message(&payload.output)));
    let input_bytes = input
        .get("content_bytes")
        .and_then(Value::as_u64)
        .map(|bytes| bytes as usize)
        .or_else(|| input.get("content").and_then(Value::as_str).map(str::len))
        .unwrap_or(0);
    let bytes = actual_bytes.unwrap_or(input_bytes);
    ToolHeaderView::new(
        format!(
            "{} {display_path} {bytes} bytes",
            tool_display_name("Write")
        ),
        vec![],
    )
}

fn edit_header(
    input: &Value,
    result_payload: Option<&ToolResultPayload>,
    workspace_root: Option<&Path>,
) -> ToolHeaderView {
    let path = file_path(input);
    let rel = display_path(path, workspace_root);
    let display_path = truncate_ellipsis_tail(&rel, PATH_TRUNCATE);
    if let Some(result) = typed_data::<sdk::tool_result::EditResult>(result_payload) {
        return ToolHeaderView::new(
            format!(
                "{} {display_path} (Replaced {})",
                tool_display_name("Edit"),
                result.replacements_made
            ),
            vec![],
        );
    }
    let old_len = input
        .get("old_string")
        .and_then(Value::as_str)
        .map(str::len)
        .unwrap_or(0);
    let new_len = input
        .get("new_string")
        .and_then(Value::as_str)
        .map(str::len)
        .unwrap_or(0);
    ToolHeaderView::new(
        format!(
            "{} {display_path} Changed {old_len} -> {new_len} chars",
            tool_display_name("Edit")
        ),
        vec![],
    )
}

fn glob_header(input: &Value, result_payload: Option<&ToolResultPayload>) -> ToolHeaderView {
    let pattern = str_arg(input, "pattern", "");
    let suffix = typed_data::<sdk::tool_result::GlobResult>(result_payload)
        .map(|result| format!(" ({} files)", result.count))
        .unwrap_or_default();
    let title = if pattern.is_empty() {
        format!("{}{}", tool_display_name("Glob"), suffix)
    } else {
        format!("{} {}{}", tool_display_name("Glob"), pattern, suffix)
    };
    ToolHeaderView::new(title, vec![])
}

fn grep_header(
    input: &Value,
    result_payload: Option<&ToolResultPayload>,
    workspace_root: Option<&Path>,
) -> ToolHeaderView {
    let pattern = str_arg(input, "pattern", "");
    let path = str_arg(input, "path", ".");
    let rel = display_path(path, workspace_root);
    let suffix = typed_data::<sdk::tool_result::GrepResult>(result_payload)
        .map(|result| format!(" ({} matches)", result.total_matches))
        .unwrap_or_default();
    let body = if pattern.is_empty() {
        format!("in {}", truncate_ellipsis_tail(&rel, 40))
    } else if result_payload.is_some() {
        format!("/{pattern}/, path={rel}")
    } else {
        format!("/{pattern}/ in {}", truncate_ellipsis_tail(&rel, 40))
    };
    ToolHeaderView::new(
        format!("{} {body}{suffix}", tool_display_name("Grep")),
        vec![],
    )
}

fn agent_header(input: &Value, result_payload: Option<&ToolResultPayload>) -> ToolHeaderView {
    let description = str_arg(input, "description", "sub-task");
    let target = typed_data::<sdk::tool_result::AgentResult>(result_payload)
        .and_then(|result| result.task_id)
        .filter(|id| !id.is_empty());
    let mut title = match target {
        Some(task_id) => format!(
            "{} {description} -> [{task_id}]",
            tool_display_name("Agent")
        ),
        None => format!("{} {description}", tool_display_name("Agent")),
    };
    if let Some(role) = input.get("role").and_then(Value::as_str) {
        title.push_str(&format!(" [role: {role}]"));
    }
    if let Some(model) = input.get("model").and_then(Value::as_str) {
        title.push_str(&format!(" [model: {model}]"));
    }
    let prompt = str_arg(input, "prompt", "");
    let details = if prompt.is_empty() {
        vec![]
    } else {
        vec![truncate_ellipsis(prompt, DETAIL_TRUNCATE)]
    };
    ToolHeaderView::new(title, details)
}

fn enter_worktree_header(
    input: &Value,
    result_payload: Option<&ToolResultPayload>,
) -> ToolHeaderView {
    if let Some(result) = typed_data::<sdk::tool_result::EnterWorktreeResult>(result_payload) {
        return ToolHeaderView::new(
            format!(
                "{} branch={} ({})",
                tool_display_name("EnterWorktree"),
                result.branch,
                result.workspace_root.display()
            ),
            vec![],
        );
    }
    let target = input
        .get("branch")
        .and_then(Value::as_str)
        .or_else(|| input.get("path").and_then(Value::as_str))
        .unwrap_or("worktree");
    ToolHeaderView::new(
        format!("{} {target}", tool_display_name("EnterWorktree")),
        vec![],
    )
}

fn exit_worktree_header(result_payload: Option<&ToolResultPayload>) -> ToolHeaderView {
    let suffix = typed_data::<sdk::tool_result::ExitWorktreeResult>(result_payload)
        .map(|result| format!(" (back to {})", result.workspace_root.display()))
        .unwrap_or_default();
    ToolHeaderView::new(
        format!("{}{}", tool_display_name("ExitWorktree"), suffix),
        vec![],
    )
}

fn web_fetch_header(input: &Value, result_payload: Option<&ToolResultPayload>) -> ToolHeaderView {
    let url = str_arg(input, "url", "");
    let suffix = typed_data::<sdk::tool_result::WebFetchResult>(result_payload)
        .filter(|result| result.truncated)
        .map(|_| " (truncated)".to_string())
        .unwrap_or_default();
    let title = if url.is_empty() {
        format!("{}{}", tool_display_name("WebFetch"), suffix)
    } else {
        format!(
            "{} {}{}",
            tool_display_name("WebFetch"),
            truncate_ellipsis(url, PATH_TRUNCATE),
            suffix
        )
    };
    ToolHeaderView::new(title, vec![])
}

fn ask_user_header(result_payload: Option<&ToolResultPayload>) -> ToolHeaderView {
    let suffix = typed_data::<sdk::tool_result::AskUserQuestionResult>(result_payload)
        .map(|result| format!(" ({} options)", result.options.len()))
        .unwrap_or_default();
    ToolHeaderView::new(
        format!("{}{}", tool_display_name("AskUserQuestion"), suffix),
        vec![],
    )
}

fn fallback_header(name: &str, input: &Value) -> ToolHeaderView {
    ToolHeaderView::new(
        tool_display_name(name).to_string(),
        vec![truncate_json_value(input)],
    )
}

fn read_range_info(input: &Value, actual_lines: Option<usize>) -> String {
    let offset = input.get("offset").and_then(Value::as_u64).unwrap_or(0) as usize;
    let limit = input.get("limit").and_then(Value::as_u64).unwrap_or(2000) as usize;
    let start = offset + 1;
    match actual_lines {
        Some(actual) => format!("{start}:{} ({actual} lines)", offset + actual),
        None if input.get("offset").is_some() || input.get("limit").is_some() => {
            format!("{start}:{}", offset + limit)
        }
        None => String::new(),
    }
}

fn file_path(input: &Value) -> &str {
    input.get("file_path").and_then(Value::as_str).unwrap_or("")
}

fn str_arg<'a>(input: &'a Value, key: &str, default: &'a str) -> &'a str {
    input.get(key).and_then(Value::as_str).unwrap_or(default)
}

fn display_path(path: &str, workspace_root: Option<&Path>) -> String {
    let Some(root) = workspace_root else {
        return path.to_string();
    };
    let absolute = Path::new(path);
    absolute
        .strip_prefix(root)
        .ok()
        .and_then(|relative| relative.to_str())
        .map(str::to_string)
        .unwrap_or_else(|| path.to_string())
}

fn typed_data<T: DeserializeOwned>(payload: Option<&ToolResultPayload>) -> Option<T> {
    payload.and_then(|payload| serde_json::from_value(payload.content.clone()).ok())
}

fn parse_line_count_from_message(message: &str) -> Option<usize> {
    let rest = message.split_once("Read ")?.1;
    let count = rest.split_once(" line")?.0;
    count.parse().ok()
}

fn parse_bytes_from_message(message: &str) -> Option<usize> {
    let rest = message.split_once("Wrote ")?.1;
    let bytes = rest.split_once(" byte")?.0;
    bytes.parse().ok()
}

fn truncate_json_value(value: &Value) -> String {
    truncate_ellipsis(&value.to_string(), 120)
}

fn truncate_ellipsis(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    let keep = max_chars.saturating_sub(3);
    let mut output: String = value.chars().take(keep).collect();
    output.push_str("...");
    output
}

fn truncate_ellipsis_tail(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    let keep = max_chars.saturating_sub(3);
    let tail: String = value
        .chars()
        .rev()
        .take(keep)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("...{tail}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn bash_header_uses_display_name_and_details() {
        let view = format_tool_header_view("Bash", &json!({"command":"cargo test"}), None, None);
        assert_eq!(view.title, "Run cargo test");
        assert_eq!(view.details, vec!["cargo test"]);
    }

    #[test]
    fn write_header_prefers_result_bytes_then_realtime_bytes() {
        let input = json!({"file_path":"out.rs","content":"short","content_bytes": 123});
        let view = format_tool_header_view("Write", &input, None, None);
        assert_eq!(view.title, "Write out.rs 123 bytes");

        let payload = ToolResultPayload::new(
            String::new(),
            json!({"file_path":"out.rs","bytes_written":456}),
            false,
            0,
        );
        let view = format_tool_header_view("Write", &input, Some(&payload), None);
        assert_eq!(view.title, "Write out.rs 456 bytes");
    }

    #[test]
    fn read_header_uses_actual_line_count() {
        let input = json!({"file_path":"src/main.rs","offset":10,"limit":20});
        let payload = ToolResultPayload::new(
            "Read 3 lines from src/main.rs".to_string(),
            json!({"file_path":"src/main.rs","line_count":3,"content":""}),
            false,
            0,
        );
        let view = format_tool_header_view("Read", &input, Some(&payload), None);
        assert_eq!(view.title, "Read src/main.rs 11:13 (3 lines)");
    }

    #[test]
    fn agent_header_includes_task_and_meta() {
        let input = json!({"description":"review","role":"reviewer","model":"m"});
        let payload = ToolResultPayload::new(
            String::new(),
            json!({"task_id":"task-1","output":"done"}),
            false,
            0,
        );
        let view = format_tool_header_view("Agent", &input, Some(&payload), None);
        assert_eq!(
            view.title,
            "Agent review -> [task-1] [role: reviewer] [model: m]"
        );
    }

    #[test]
    fn bash_header_includes_non_zero_exit() {
        let input = json!({"command":"cargo test"});
        let payload = ToolResultPayload::new(
            String::new(),
            json!({"stdout":"","stderr":"fail","exit_code":2,"signal":null}),
            true,
            0,
        );
        let view = format_tool_header_view("Bash", &input, Some(&payload), None);
        assert_eq!(view.title, "Run cargo test (exit 2)");
    }

    #[test]
    fn glob_and_grep_headers_include_result_counts() {
        let glob_payload = ToolResultPayload::new(
            String::new(),
            json!({"files":["a.rs","b.rs"],"count":2}),
            false,
            0,
        );
        let glob = format_tool_header_view(
            "Glob",
            &json!({"pattern":"**/*.rs"}),
            Some(&glob_payload),
            None,
        );
        assert_eq!(glob.title, "Find **/*.rs (2 files)");

        let grep_payload = ToolResultPayload::new(
            String::new(),
            json!({"matches":[],"total_matches":7,"shown":5,"query":"foo"}),
            false,
            0,
        );
        let grep = format_tool_header_view(
            "Grep",
            &json!({"pattern":"foo","path":"src"}),
            Some(&grep_payload),
            None,
        );
        assert_eq!(grep.title, "Search /foo/, path=src (7 matches)");
    }

    #[test]
    fn edit_and_worktree_headers_include_result_payload() {
        let edit_payload = ToolResultPayload::new(
            String::new(),
            json!({
                "file_path":"src/lib.rs",
                "replacements_made":3,
                "dry_run":false,
                "old":"a",
                "new":"b",
                "start_line":1
            }),
            false,
            0,
        );
        let edit = format_tool_header_view(
            "Edit",
            &json!({"file_path":"src/lib.rs","old_string":"a","new_string":"b"}),
            Some(&edit_payload),
            None,
        );
        assert_eq!(edit.title, "Edit src/lib.rs (Replaced 3)");

        let enter_payload = ToolResultPayload::new(
            String::new(),
            json!({
                "branch":"fix/demo",
                "path_base":"/repo/.worktrees/fix-demo",
                "workspace_root":"/repo/.worktrees/fix-demo",
                "guidance":""
            }),
            false,
            0,
        );
        let enter = format_tool_header_view(
            "EnterWorktree",
            &json!({"branch":"fix/demo"}),
            Some(&enter_payload),
            None,
        );
        assert_eq!(
            enter.title,
            "Enter Worktree branch=fix/demo (/repo/.worktrees/fix-demo)"
        );
    }

    #[test]
    fn unknown_tool_has_fallback_detail() {
        let view = format_tool_header_view("UnknownTool", &json!({"key":"value"}), None, None);
        assert_eq!(view.title, "UnknownTool");
        assert_eq!(view.details, vec!["{\"key\":\"value\"}"]);
    }
}
