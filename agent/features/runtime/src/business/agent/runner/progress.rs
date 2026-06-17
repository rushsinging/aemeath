use crate::business::agent::ToolCall;
use share::tool::{AgentProgressEvent, AgentProgressKind, AgentToolCallProgress};

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
                    id: call.id.to_string(),
                    name: call.name.clone(),
                    input: call.input.clone(),
                })
                .collect(),
        },
    }
}

#[cfg(test)]
pub(crate) fn format_grouped_tool_summaries(tool_calls: &[ToolCall]) -> String {
    let mut counts: Vec<(&str, usize)> = Vec::new();
    for call in tool_calls {
        if let Some(entry) = counts
            .iter_mut()
            .find(|(name, _)| *name == call.name.as_str())
        {
            entry.1 += 1;
        } else {
            counts.push((call.name.as_str(), 1));
        }
    }

    counts
        .into_iter()
        .map(|(name, count)| {
            if count > 1 {
                format!("{name} ×{count}")
            } else {
                name.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" | ")
}
