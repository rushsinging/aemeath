use sdk::{AgentProgressEventView, AgentProgressKindView};
use serde_json::Value;

/// 把 AgentProgressEventView 格式化为人类可读消息，供 TUI activities 渲染。
pub(super) fn format_agent_progress<F>(
    event: &AgentProgressEventView,
    mut format_tool_header: F,
) -> String
where
    F: FnMut(&str, &Value) -> String,
{
    match &event.kind {
        AgentProgressKindView::Started { .. } => String::new(),
        AgentProgressKindView::Message { text } => complete_progress_message(text),
        AgentProgressKindView::ToolOutput { .. } => String::new(),
        AgentProgressKindView::ToolCalls { calls } => {
            if calls.is_empty() {
                return String::new();
            }
            let lines: Vec<String> = calls
                .iter()
                .map(|tc| format!("→ {}", format_tool_header(&tc.name, &tc.input)))
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
