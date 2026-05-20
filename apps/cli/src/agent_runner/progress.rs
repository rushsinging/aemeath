use aemeath_core::agent::ToolCall;
use aemeath_core::tool::{AgentProgressEvent, AgentProgressKind, AgentToolCallProgress};

pub(crate) fn build_tool_calls_progress_event(
    sequence: usize,
    tool_calls: &[ToolCall],
) -> AgentProgressEvent {
    AgentProgressEvent {
        sequence,
        kind: AgentProgressKind::ToolCalls {
            calls: tool_calls
                .iter()
                .map(|call| AgentToolCallProgress {
                    id: call.id.clone(),
                    name: call.name.clone(),
                    input: call.input.clone(),
                    summary: summarize_tool_input(&call.name, &call.input),
                })
                .collect(),
        },
    }
}

#[cfg(test)]
pub(crate) fn format_grouped_tool_summaries(tool_calls: &[ToolCall]) -> String {
    let mut grouped: Vec<(&str, Vec<String>)> = Vec::new();
    for call in tool_calls {
        if let Some((_, summaries)) = grouped.iter_mut().find(|(name, _)| *name == call.name) {
            summaries.push(summarize_tool_input(&call.name, &call.input));
        } else {
            grouped.push((
                call.name.as_str(),
                vec![summarize_tool_input(&call.name, &call.input)],
            ));
        }
    }

    grouped
        .into_iter()
        .map(|(name, summaries)| {
            let count = summaries.len();
            let visible = summaries
                .iter()
                .filter(|summary| !summary.is_empty())
                .take(3)
                .cloned()
                .collect::<Vec<_>>();
            let suffix = if visible.is_empty() {
                String::new()
            } else {
                let mut text = visible.join(", ");
                if count > 3 {
                    text.push_str(&format!(" +{} more", count - 3));
                }
                format!(": {text}")
            };
            if count > 1 {
                format!("{name} ×{count}{suffix}")
            } else {
                format!("{name}{suffix}")
            }
        })
        .collect::<Vec<_>>()
        .join(" | ")
}

fn summarize_tool_input(name: &str, input: &serde_json::Value) -> String {
    match name {
        "Read" | "Write" | "Edit" | "LSP" => extract_display_path(input, &["file_path", "path"]),
        "Grep" => {
            let pattern = input.get("pattern").and_then(|v| v.as_str()).unwrap_or("");
            let path = extract_display_path(input, &["path"]);
            match (pattern.is_empty(), path.is_empty()) {
                (false, false) => {
                    format!("\"{}\" in {}", truncate_progress_part(pattern, 48), path)
                }
                (false, true) => format!("\"{}\"", truncate_progress_part(pattern, 48)),
                (true, false) => path,
                (true, true) => fallback_json_summary(input),
            }
        }
        "Glob" => input
            .get("pattern")
            .and_then(|v| v.as_str())
            .map(|pattern| truncate_progress_part(pattern, 72))
            .unwrap_or_else(|| fallback_json_summary(input)),
        "Bash" => input
            .get("command")
            .and_then(|v| v.as_str())
            .map(|command| truncate_progress_part(command, 32))
            .unwrap_or_else(|| fallback_json_summary(input)),
        "WebFetch" => input
            .get("url")
            .and_then(|v| v.as_str())
            .map(|url| truncate_progress_part(url, 72))
            .unwrap_or_else(|| fallback_json_summary(input)),
        "TaskUpdate" | "TaskGet" | "TaskOutput" | "TaskStop" => input
            .get("taskId")
            .and_then(|v| v.as_str())
            .map(|id| truncate_progress_part(id, 48))
            .unwrap_or_else(|| fallback_json_summary(input)),
        "TaskCreate" => input
            .get("subject")
            .and_then(|v| v.as_str())
            .map(|subject| truncate_progress_part(subject, 72))
            .unwrap_or_else(|| fallback_json_summary(input)),
        "Memory" => input
            .get("action")
            .and_then(|v| v.as_str())
            .map(ToString::to_string)
            .unwrap_or_else(|| fallback_json_summary(input)),
        "Skill" => input
            .get("skill")
            .and_then(|v| v.as_str())
            .map(|skill| truncate_progress_part(skill, 72))
            .unwrap_or_else(|| fallback_json_summary(input)),
        _ => fallback_json_summary(input),
    }
}

fn extract_display_path(input: &serde_json::Value, keys: &[&str]) -> String {
    keys.iter()
        .find_map(|key| input.get(*key).and_then(|v| v.as_str()))
        .map(|path| {
            let trimmed = path.trim_start_matches("/repo/");
            let components = trimmed.split('/').collect::<Vec<_>>();
            let compact = if components.len() > 3 {
                components[components.len() - 3..].join("/")
            } else {
                trimmed.to_string()
            };
            truncate_progress_part(&compact, 72)
        })
        .unwrap_or_default()
}

fn fallback_json_summary(input: &serde_json::Value) -> String {
    truncate_progress_part(&input.to_string(), 72)
}

fn truncate_progress_part(text: &str, max_chars: usize) -> String {
    let count = text.chars().count();
    if count <= max_chars {
        return text.to_string();
    }
    let mut truncated = text.chars().take(max_chars).collect::<String>();
    if let Some(idx) = truncated.rfind(" && ") {
        truncated.truncate(idx);
    }
    format!("{truncated}…")
}
