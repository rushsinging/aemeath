use crate::application::tool::agent::ToolCall;
use tools::{
    AgentProgressEvent, AgentProgressKind, AgentProgressSourceContext, AgentToolCallProgress,
};

pub(crate) fn build_progress_event(
    source_context: AgentProgressSourceContext,
    sequence: usize,
    kind: AgentProgressKind,
) -> AgentProgressEvent {
    AgentProgressEvent {
        source_context: Some(source_context),
        sequence,
        kind,
    }
}

pub(crate) fn build_tool_calls_progress_event(
    source_context: AgentProgressSourceContext,
    sequence: usize,
    tool_calls: &[ToolCall],
) -> AgentProgressEvent {
    build_progress_event(
        source_context,
        sequence,
        AgentProgressKind::ToolCalls {
            calls: tool_calls
                .iter()
                .map(|call| AgentToolCallProgress {
                    id: call.id.to_string(),
                    name: call.name.clone(),
                    input: call.input.clone(),
                })
                .collect(),
        },
    )
}
