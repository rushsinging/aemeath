use crate::tui::view_model::tool_name::tool_display_name;
use sdk::{AgentProgressEventView, AgentProgressKindView};
use serde_json::Value;

/// 把 AgentProgressEventView 格式化为人类可读消息，供 TUI activities 渲染。
pub(super) fn format_agent_progress(event: &AgentProgressEventView) -> String {
    match &event.kind {
        AgentProgressKindView::Started { .. } => String::new(),
        AgentProgressKindView::Message { text } => complete_progress_message(text),
        AgentProgressKindView::ToolCalls { calls } => {
            if calls.is_empty() {
                return String::new();
            }
            let lines: Vec<String> = calls
                .iter()
                .map(|tc| format_agent_tool_call(&tc.name, &tc.input))
                .collect();
            complete_progress_message(&lines.join("\n"))
        }
    }
}

fn complete_progress_message(message: &str) -> String {
    if message.is_empty() || message.ends_with('\n') {
        message.to_string()
    } else {
        format!("{message}\n")
    }
}

fn format_agent_tool_call(name: &str, input: &Value) -> String {
    let display_name = tool_display_name(name);
    let arg = match name {
        "Read" => format_read_header_arg(input),
        "Glob" => str_arg(input, "pattern").to_string(),
        "Grep" => format_grep_header_arg(input),
        "Bash" => str_arg(input, "command").to_string(),
        "Write" | "Edit" => str_arg(input, "file_path").to_string(),
        _ => fallback_input_preview(input),
    };
    if arg.is_empty() {
        format!("→ {display_name}")
    } else {
        format!("→ {display_name} {arg}")
    }
}

fn format_read_header_arg(input: &Value) -> String {
    let path = str_arg(input, "file_path");
    if path.is_empty() {
        return String::new();
    }
    let offset = input.get("offset").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
    let limit = input.get("limit").and_then(|v| v.as_u64()).unwrap_or(2000) as usize;
    let has_explicit = input.get("offset").is_some() || input.get("limit").is_some();
    if has_explicit {
        format!("{path} {}:{}", offset + 1, offset + limit)
    } else {
        path.to_string()
    }
}

fn format_grep_header_arg(input: &Value) -> String {
    let pattern = str_arg(input, "pattern");
    let path = str_arg(input, "path");
    let path = if path.is_empty() { "." } else { path };
    if pattern.is_empty() {
        format!("in {path}")
    } else {
        format!("/{pattern}/, path={path}")
    }
}

fn str_arg<'a>(input: &'a Value, key: &str) -> &'a str {
    input.get(key).and_then(|v| v.as_str()).unwrap_or("")
}

fn fallback_input_preview(input: &Value) -> String {
    let raw = match input {
        Value::String(s) => s.clone(),
        value => value.to_string(),
    };
    truncate_ellipsis(&raw, 100)
}

fn truncate_ellipsis(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let keep = max_chars.saturating_sub(3);
    let mut output: String = text.chars().take(keep).collect();
    output.push_str("...");
    output
}
