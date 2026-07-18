use crate::application::agent::ToolCall;
use tools::{AgentProgressEvent, AgentProgressKind, AgentToolCallProgress};

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
