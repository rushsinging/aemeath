use crate::tui::model::conversation::agent_progress::AgentActivityLine;
use sdk::{AgentProgressEventView, AgentProgressKindView};
use serde_json::Value;

/// 将 AgentProgress 保持为 typed activity；marker 由 renderer 统一添加。
pub(super) fn format_agent_progress<F>(
    event: &AgentProgressEventView,
    mut format_tool_header: F,
) -> Vec<AgentActivityLine>
where
    F: FnMut(&str, &Value) -> String,
{
    match &event.kind {
        AgentProgressKindView::Started { .. } | AgentProgressKindView::ToolOutput { .. } => {
            Vec::new()
        }
        AgentProgressKindView::Message { text } => split_activity_lines(text)
            .into_iter()
            .map(AgentActivityLine::message)
            .collect(),
        AgentProgressKindView::ToolCalls { calls } => calls
            .iter()
            .map(|tool_call| {
                AgentActivityLine::tool_call(format_tool_header(&tool_call.name, &tool_call.input))
            })
            .collect(),
    }
}

fn split_activity_lines(message: &str) -> Vec<String> {
    message
        .lines()
        .map(str::to_string)
        .filter(|line| !line.is_empty())
        .collect()
}
