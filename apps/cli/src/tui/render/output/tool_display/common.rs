use sdk::AgentToolCallProgressView;

use crate::tui::render::display::safe_text;

pub(super) fn str_arg<'a>(input: &'a serde_json::Value, key: &str, default: &'a str) -> &'a str {
    input
        .get(key)
        .and_then(|value| value.as_str())
        .unwrap_or(default)
}

pub(super) fn u64_arg(input: &serde_json::Value, key: &str) -> Option<u64> {
    input.get(key).and_then(|value| value.as_u64())
}

pub(super) fn bool_arg(input: &serde_json::Value, key: &str, default: bool) -> bool {
    input
        .get(key)
        .and_then(|value| value.as_bool())
        .unwrap_or(default)
}

pub(super) fn file_path(input: &serde_json::Value) -> &str {
    str_arg(input, "file_path", "?")
}

pub(super) fn truncate_ellipsis(text: &str, max_width: usize) -> String {
    if text.len() > max_width {
        let (prefix, _) = safe_text::truncate_unicode_width(text, max_width);
        format!("{}...", prefix)
    } else {
        text.to_string()
    }
}

pub(super) fn truncate_json(raw: &str) -> String {
    truncate_ellipsis(raw, 100)
}

pub(super) fn format_todowrite_value(input: &serde_json::Value) -> Option<(String, Vec<String>)> {
    let todos = input.get("todos").and_then(|todos| todos.as_array())?;
    let count = todos.len();
    let mut details = Vec::new();

    for todo in todos.iter().take(3) {
        let subject = str_arg(todo, "subject", "?");
        let status = str_arg(todo, "status", "pending");
        let icon = match status {
            "completed" => "✓",
            "in_progress" => "◐",
            _ => "○",
        };
        details.push(format!("{icon} {subject}"));
    }

    if count > 3 {
        details.push(format!("... +{} more", count - 3));
    }

    Some((format!("● TodoWrite ({count} items)"), details))
}

pub(super) fn format_agent_tool_calls(calls: &[AgentToolCallProgressView]) -> String {
    let mut grouped: Vec<(&str, Vec<&str>)> = Vec::new();
    for call in calls {
        if let Some((_, summaries)) = grouped.iter_mut().find(|(name, _)| *name == call.name) {
            summaries.push(call.summary.as_str());
        } else {
            grouped.push((call.name.as_str(), vec![call.summary.as_str()]));
        }
    }

    grouped
        .into_iter()
        .map(|(name, summaries)| format_tool_group(name, &summaries))
        .collect::<Vec<_>>()
        .join(" | ")
}

fn format_tool_group(name: &str, summaries: &[&str]) -> String {
    let count = summaries.len();
    let visible = summaries
        .iter()
        .filter(|summary| !summary.is_empty())
        .take(3)
        .copied()
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
}
