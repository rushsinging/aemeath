use crate::tui::adapter::hook_notice::hook_event_notice;
use crate::tui::app::event::{StatusContextUpdate, UiEvent};
use crate::tui::effect::effect::Effect;
use crate::tui::model::conversation::intent::*;
use crate::tui::model::conversation::tool_call::ToolCallStatus;
use crate::tui::model::diagnostic::intent::DiagnosticIntent;
use crate::tui::model::diagnostic::notice::DiagnosticSeverity;
use crate::tui::model::runtime::session_intent::SessionIntent;

mod progress;
mod sanitize;

#[cfg(test)]
mod tests;

use progress::format_agent_progress;
use sanitize::{
    json_value_kind, sanitize_tool_arguments_delta, sanitize_tool_output,
    sanitize_tool_result_content,
};

#[derive(Debug, Default, PartialEq)]
pub struct AgentEventMapping {
    pub conversation: Vec<ConversationIntent>,
    pub diagnostic: Vec<DiagnosticIntent>,
    pub session: Vec<SessionIntent>,
    pub effects: Vec<Effect>,
}

fn default_subagent_tool_header(name: &str, input: &serde_json::Value) -> String {
    let raw = match input {
        serde_json::Value::String(s) => s.clone(),
        value => value.to_string(),
    };
    if raw.is_empty() {
        crate::tui::view_model::tool_name::tool_display_name(name).to_string()
    } else {
        format!(
            "{} {}",
            crate::tui::view_model::tool_name::tool_display_name(name),
            truncate_agent_progress_json(&raw)
        )
    }
}

fn truncate_agent_progress_json(raw: &str) -> String {
    const MAX_CHARS: usize = 100;
    if raw.chars().count() <= MAX_CHARS {
        return raw.to_string();
    }
    let mut output: String = raw.chars().take(MAX_CHARS.saturating_sub(3)).collect();
    output.push_str("...");
    output
}

fn tool_call_status_from_sdk(status: sdk::ToolCallStatusView) -> ToolCallStatus {
    match status {
        sdk::ToolCallStatusView::PendingArgs => ToolCallStatus::PendingArgs,
        sdk::ToolCallStatusView::Ready => ToolCallStatus::Ready,
        sdk::ToolCallStatusView::Running => ToolCallStatus::Running,
    }
}

pub fn map_agent_event(event: &UiEvent) -> AgentEventMapping {
    map_agent_event_with_tool_header(event, default_subagent_tool_header)
}

pub fn map_agent_event_with_tool_header<F>(
    event: &UiEvent,
    mut format_subagent_tool_header: F,
) -> AgentEventMapping
where
    F: FnMut(&str, &serde_json::Value) -> String,
{
    match event {
        // ── Runtime observations → ConversationIntent (inlined from ToolFlowProjector) ──
        UiEvent::Text { context, text } => {
            clear_placeholder_then(ConversationIntent::AssistantText(AssistantText {
                chat_id: context.chat_id.clone(),
                turn_id: context.turn_id.clone(),
                text: text.clone(),
            }))
        }
        UiEvent::Thinking { context, text } => {
            clear_placeholder_then(ConversationIntent::ThinkingText(ThinkingText {
                chat_id: context.chat_id.clone(),
                turn_id: context.turn_id.clone(),
                text: text.clone(),
            }))
        }
        UiEvent::BlockComplete { context, .. } => {
            conversation(ConversationIntent::CompleteBlock(CompleteBlock {
                chat_id: context.chat_id.clone(),
                turn_id: context.turn_id.clone(),
            }))
        }
        UiEvent::ToolCallStart {
            context,
            id,
            provider_id,
            name,
            index,
        } => {
            crate::tui::log_debug!(
                "map tool_call_start chat_id={} turn_id={} id={} provider_id={:?} name={} index={}",
                context.chat_id,
                context.turn_id,
                id,
                provider_id,
                name,
                index,
            );
            clear_placeholder_then(ConversationIntent::ToolCallStart(ToolCallStart {
                chat_id: context.chat_id.clone(),
                turn_id: context.turn_id.clone(),
                id: id.clone(),
                provider_id: provider_id.clone(),
                name: name.clone(),
                index: *index,
            }))
        }
        UiEvent::ToolCallUpdate {
            context,
            id,
            provider_id,
            name,
            index,
            arguments_delta,
            arguments,
            status,
        } => {
            let args = arguments_delta
                .clone()
                .or_else(|| arguments.as_ref().map(|value| value.to_string()));
            crate::tui::log_debug!(
                "map tool_call_update chat_id={} turn_id={} id={} provider_id={:?} name={} index={} args_len={}",
                context.chat_id,
                context.turn_id,
                id,
                provider_id,
                name,
                index,
                args.as_ref().map(|s| s.len()).unwrap_or(0),
            );
            clear_placeholder_then(ConversationIntent::ToolCallUpdate(ToolCallUpdate {
                chat_id: context.chat_id.clone(),
                turn_id: context.turn_id.clone(),
                id: id.clone(),
                provider_id: provider_id.clone(),
                name: name.clone(),
                index: *index,
                arguments: args
                    .as_ref()
                    .map(|value| sanitize_tool_arguments_delta(name, value)),
                status: tool_call_status_from_sdk(*status),
            }))
        }
        UiEvent::ToolResult {
            context,
            id,
            provider_id,
            tool_name,
            output,
            content,
            is_error,
            images,
        } => {
            crate::tui::log_debug!(
                "map tool_result chat_id={} turn_id={} id={} provider_id={} tool_name={} output_len={} content_kind={} is_error={} image_count={}",
                context.chat_id,
                context.turn_id,
                id,
                provider_id,
                tool_name,
                output.len(),
                json_value_kind(content),
                is_error,
                images.len(),
            );
            clear_placeholder_then(ConversationIntent::ToolResult(ToolResult {
                chat_id: context.chat_id.clone(),
                turn_id: context.turn_id.clone(),
                id: id.clone(),
                provider_id: provider_id.clone(),
                tool_name: tool_name.clone(),
                output: sanitize_tool_output(tool_name, output),
                content: sanitize_tool_result_content(tool_name, content.clone()),
                is_error: *is_error,
                image_count: images.len(),
            }))
        }
        UiEvent::AgentProgress {
            context,
            tool_id,
            event,
        } => match &event.kind {
            sdk::AgentProgressKindView::Started { role, model } => {
                conversation(ConversationIntent::UpdateAgentMeta(UpdateAgentMeta {
                    chat_id: context.chat_id.clone(),
                    turn_id: context.turn_id.clone(),
                    tool_id: tool_id.clone(),
                    role: role.clone(),
                    model: model.clone(),
                }))
            }
            sdk::AgentProgressKindView::ToolOutput { .. } => AgentEventMapping::default(),
            _ => conversation(ConversationIntent::RecordAgentProgress(
                RecordAgentProgress {
                    chat_id: context.chat_id.clone(),
                    turn_id: context.turn_id.clone(),
                    tool_id: tool_id.clone(),
                    message: format_agent_progress(&event, &mut format_subagent_tool_header),
                },
            )),
        },
        UiEvent::Done { context }
        | UiEvent::DoneWithDuration { context, .. }
        | UiEvent::Cancelled { context } => {
            conversation(ConversationIntent::CompleteChat(CompleteChat {
                chat_id: context.chat_id.clone(),
                turn_id: context.turn_id.clone(),
            }))
        }

        // ── Usage / LiveTps → ConversationIntent ──
        UiEvent::Usage {
            input,
            output,
            last_input,
            elapsed_secs,
        } => {
            let mut intents = vec![ConversationIntent::RecordUsage(RecordUsage {
                input_tokens: u64::from(*input),
                output_tokens: u64::from(*output),
                last_input_tokens: u64::from(*last_input),
                cost_usd: 0.0,
            })];
            if *elapsed_secs > 0.0 {
                intents.push(ConversationIntent::RecordLiveTps(RecordLiveTps {
                    tps: f64::from(*output) / elapsed_secs,
                }));
            }
            AgentEventMapping {
                conversation: intents,
                ..AgentEventMapping::default()
            }
        }
        UiEvent::LiveTps(tps) => conversation(ConversationIntent::RecordLiveTps(RecordLiveTps {
            tps: *tps,
        })),

        // ── Error ──
        UiEvent::Error(message) => {
            let mut mapping = conversation(ConversationIntent::AppendError(AppendError {
                text: message.clone(),
            }));
            mapping.diagnostic.push(DiagnosticIntent::RecordNotice {
                severity: DiagnosticSeverity::Error,
                message: message.clone(),
            });
            mapping.effects.push(Effect::RunHook {
                name: "error".to_string(),
                message: message.clone(),
            });
            mapping
        }

        // ── System messages ──
        UiEvent::SystemMessage(text) | UiEvent::ReminderRecap(text) => conversation(
            ConversationIntent::AppendSystemMessage(AppendSystemMessage { text: text.clone() }),
        ),
        UiEvent::ModelStreamWaiting {
            context,
            elapsed_secs,
            phase,
        } => conversation(ConversationIntent::UpsertModelStreamPlaceholder(
            UpsertModelStreamPlaceholder {
                placeholder: crate::tui::app::event::ModelStreamWaitingView {
                    context: context.clone(),
                    elapsed_secs: *elapsed_secs,
                    phase: phase.clone(),
                },
            },
        )),
        UiEvent::TurnStarted { messages }
        | UiEvent::MicrocompactDone { messages, .. }
        | UiEvent::StopHookBlocked { messages }
        | UiEvent::PostToolExecutionSync { messages }
        | UiEvent::CompactRollback { messages }
        | UiEvent::CompactFinished { messages } => session(SessionIntent::MessagesSynced {
            message_count: messages.len(),
        }),
        UiEvent::ApiError { messages, .. } => session(SessionIntent::MessagesSynced {
            message_count: messages.len(),
        }),
        UiEvent::AskUserBatch { .. } => AgentEventMapping::default(),

        // ── HookEvent → notice via conversation ──
        UiEvent::HookEvent(event) => {
            if event.hook_name == "PostCompact" {
                return AgentEventMapping::default();
            }
            let mut mapping = AgentEventMapping::default();
            if let Some(notice) = hook_event_notice(event) {
                mapping
                    .conversation
                    .push(ConversationIntent::AppendHookNotice(AppendHookNotice {
                        content: notice,
                    }));
            }
            mapping
        }
        UiEvent::WorkingDirectoryChanged(update) => map_status_context(update),
        _ => AgentEventMapping::default(),
    }
}

fn map_status_context(update: &StatusContextUpdate) -> AgentEventMapping {
    conversation(ConversationIntent::WorkspaceSnapshotReceived(
        WorkspaceSnapshotReceived {
            path_base: Some(update.path_base.clone()),
            workspace_root: Some(update.workspace_root.clone()),
            branch: update.branch.clone(),
            kind: update.kind,
        },
    ))
}

// ════════════════════════════════════════════════════════════════════
//  Helpers — AgentEventMapping constructors
// ════════════════════════════════════════════════════════════════════

fn clear_placeholder_then(intent: ConversationIntent) -> AgentEventMapping {
    AgentEventMapping {
        conversation: vec![
            ConversationIntent::ClearModelStreamPlaceholder(ClearModelStreamPlaceholder),
            intent,
        ],
        ..AgentEventMapping::default()
    }
}

fn conversation(intent: ConversationIntent) -> AgentEventMapping {
    AgentEventMapping {
        conversation: vec![intent],
        ..AgentEventMapping::default()
    }
}

fn _diagnostic(intent: DiagnosticIntent) -> AgentEventMapping {
    AgentEventMapping {
        diagnostic: vec![intent],
        ..AgentEventMapping::default()
    }
}

fn session(intent: SessionIntent) -> AgentEventMapping {
    AgentEventMapping {
        session: vec![intent],
        ..AgentEventMapping::default()
    }
}
