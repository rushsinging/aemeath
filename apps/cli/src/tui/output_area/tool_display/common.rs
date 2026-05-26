use ::runtime::api::core::tool::AgentToolCallProgress;

use crate::tui::display::safe_text;

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

pub(super) fn format_agent_tool_calls(calls: &[AgentToolCallProgress]) -> String {
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

/// Extract a short preview string from partial JSON arguments for a given tool name.
/// Returns empty string if no useful preview can be extracted yet.
pub(super) fn extract_tool_preview(name: &str, partial_args: &str) -> String {
    // Key parameter names for each tool — the first found non-empty value wins.
    let keys: &[&str] = match name {
        "Read" | "ReadFile" => &["file_path", "path"],
        "Edit" | "EditFile" => &["file_path", "path"],
        "Write" | "WriteFile" => &["file_path", "path"],
        "Bash" => &["command"],
        "Grep" => &["pattern"],
        "Glob" => &["pattern"],
        "Agent" => &["description"],
        _ => &[],
    };
    if keys.is_empty() {
        return String::new();
    }

    // Try to extract value from partial JSON using simple string scanning.
    // This is faster and more tolerant than full JSON parsing for incomplete JSON.
    for &key in keys {
        if let Some(value) = extract_string_value(partial_args, key) {
            return truncate_ellipsis(&value, 80);
        }
    }
    String::new()
}

/// Extract a string value from (possibly incomplete) JSON by scanning for `"key":"value"`.
/// Handles escaped quotes and partial values (value may be cut off at the end).
fn extract_string_value(json: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    let start = json.find(&needle)?;
    // Skip past "key"
    let after_key = start + needle.len();
    let rest = json.get(after_key..)?;
    // Skip optional whitespace and colon
    let mut pos = 0;
    let chars: Vec<char> = rest.chars().collect();
    while pos < chars.len()
        && (chars[pos] == ' '
            || chars[pos] == ':'
            || chars[pos] == '\t'
            || chars[pos] == '\n'
            || chars[pos] == '\r')
    {
        pos += 1;
    }
    // Expect opening quote
    if pos >= chars.len() || chars[pos] != '"' {
        return None;
    }
    pos += 1;
    // Read until closing unescaped quote
    let mut result = String::new();
    while pos < chars.len() {
        let ch = chars[pos];
        if ch == '\\' && pos + 1 < chars.len() {
            // Escaped character
            pos += 1;
            result.push(chars[pos]);
        } else if ch == '"' {
            // Closing quote — complete value
            return Some(result);
        } else {
            result.push(ch);
        }
        pos += 1;
    }
    // Reached end of string without closing quote — partial value, still useful
    if result.is_empty() {
        None
    } else {
        Some(result)
    }
}
