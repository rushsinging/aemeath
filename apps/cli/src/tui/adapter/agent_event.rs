use crate::tui::adapter::runtime_view::TuiMessageSource;
use crate::tui::adapter::tui_runtime_event::{
    TuiAgentProgressKind, TuiHookStatus, TuiRuntimeEvent, TuiToolCallStatus,
};
use crate::tui::app::event::{StatusContextUpdate, UiEvent};
use crate::tui::model::conversation::ids::ToolCallId;
use crate::tui::model::conversation::intent::*;
use crate::tui::model::conversation::stop_hook_notice::stop_hook_notice_content;
use crate::tui::model::conversation::system_reminder::strip_system_reminder_envelope;
use crate::tui::model::conversation::tool_call::ToolCallStatus;
use crate::tui::model::diagnostic::intent::DiagnosticIntent;
use crate::tui::model::diagnostic::notice::DiagnosticSeverity;
use crate::tui::model::runtime::session_intent::SessionIntent;
use crate::tui::model::workspace_provider::WorkspaceIntent;

mod progress;
mod sanitize;

#[cfg(test)]
#[path = "agent_event_runtime_tests.rs"]
mod runtime_tests;
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
    pub workspace: Vec<WorkspaceIntent>,
}

#[cfg(test)]
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

#[cfg(test)]
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

#[cfg(test)]
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
                id: ToolCallId::new(id.as_str()),
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
                id: ToolCallId::new(id.as_str()),
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
                id: ToolCallId::new(id.as_str()),
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
                    tool_id: ToolCallId::new(tool_id.as_str()),
                    role: role.clone(),
                    model: model.clone(),
                }))
            }
            sdk::AgentProgressKindView::ToolOutput { .. } => AgentEventMapping::default(),
            _ => {
                let message = format_agent_progress(event, &mut format_subagent_tool_header);
                let preview: String = message.chars().take(200).collect();
                crate::tui::log_debug!(
                    "agent_progress_format kind={} seq={} msg_len={} msg={:?}",
                    format!("{:?}", event.kind).split('{').next().unwrap_or("?"),
                    event.sequence,
                    message.len(),
                    preview,
                );
                conversation(ConversationIntent::RecordAgentProgress(
                    RecordAgentProgress {
                        chat_id: context.chat_id.clone(),
                        turn_id: context.turn_id.clone(),
                        tool_id: ToolCallId::new(tool_id.as_str()),
                        message,
                    },
                ))
            }
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
            mapping
        }

        // ── System messages ──
        // runtime 允许发空 SystemMessage——hook 的 additional_context / system_message
        // 只判 `Option` 不判空串（`looping/tools.rs`、`post_batch.rs`、`compact.rs`）。
        // ACL 层在此丢弃，否则空 block 会各吃掉 2 行（空内容 + depth0 前置空行，#1106）。
        // 判空前先剥离信封：`<system-reminder></system-reminder>` 剥离后即为空。
        // 启动横幅经 `seed_banner` 直接注入 model，不走本路径，其空行不受影响。
        UiEvent::SystemMessage(text) => {
            if strip_system_reminder_envelope(text).trim().is_empty() {
                crate::tui::log_debug!(
                    "drop empty system message from runtime raw_len={}",
                    text.len()
                );
                return AgentEventMapping::default();
            }
            conversation(ConversationIntent::AppendSystemMessage(
                AppendSystemMessage { text: text.clone() },
            ))
        }
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
        | UiEvent::PostToolExecutionSync { messages }
        | UiEvent::CompactRollback { messages }
        | UiEvent::CompactFinished { messages } => session(SessionIntent::MessagesSynced {
            message_count: messages.len(),
        }),
        UiEvent::StopHookBlocked { messages } => {
            let mut mapping = session(SessionIntent::MessagesSynced {
                message_count: messages.len(),
            });
            if let Some(message) = messages
                .iter()
                .rev()
                .find(|message| message.source == TuiMessageSource::StopHook)
            {
                mapping
                    .conversation
                    .push(ConversationIntent::AppendHookNotice(AppendHookNotice {
                        content: stop_hook_notice_content(message),
                    }));
            }
            mapping
        }
        UiEvent::ApiError { messages, .. } => session(SessionIntent::MessagesSynced {
            message_count: messages.len(),
        }),

        // ── HookEvent → notice via conversation ──
        UiEvent::HookEvent(_) => AgentEventMapping::default(),
        UiEvent::HookMessage(_) => AgentEventMapping::default(),
        UiEvent::WorkingDirectoryChanged(update) => map_status_context(update),
        UiEvent::WorkspaceMetadataResolved(metadata) => AgentEventMapping {
            workspace: vec![WorkspaceIntent::ApplyMetadata {
                root: metadata.root.clone(),
                revision: metadata.revision,
                branch: metadata.branch.clone(),
                kind: metadata.kind,
            }],
            ..AgentEventMapping::default()
        },
        _ => AgentEventMapping::default(),
    }
}

pub fn map_runtime_event(event: &TuiRuntimeEvent) -> AgentEventMapping {
    use crate::tui::adapter::tui_runtime_event::{
        TuiInteractionBody, TuiRunEvent, TuiRunStepEvent,
    };
    use crate::tui::model::conversation::interaction::{
        InteractionBody, InteractionRequest, UiApprovalPrompt, UiPlanApprovalPrompt, UiRiskLevel,
        UiStuckDiagnostic, UiUserQuestion,
    };

    match event {
        TuiRuntimeEvent::Text { context, text } => {
            clear_placeholder_then(ConversationIntent::AssistantText(AssistantText {
                chat_id: crate::tui::model::conversation::ids::ChatId::new(&context.chat_id),
                turn_id: crate::tui::model::conversation::ids::ChatTurnId::new(&context.turn_id),
                text: text.clone(),
            }))
        }
        TuiRuntimeEvent::Thinking { context, text } => {
            clear_placeholder_then(ConversationIntent::ThinkingText(ThinkingText {
                chat_id: crate::tui::model::conversation::ids::ChatId::new(&context.chat_id),
                turn_id: crate::tui::model::conversation::ids::ChatTurnId::new(&context.turn_id),
                text: text.clone(),
            }))
        }
        TuiRuntimeEvent::BlockComplete { context, .. } => {
            conversation(ConversationIntent::CompleteBlock(CompleteBlock {
                chat_id: crate::tui::model::conversation::ids::ChatId::new(&context.chat_id),
                turn_id: crate::tui::model::conversation::ids::ChatTurnId::new(&context.turn_id),
            }))
        }
        TuiRuntimeEvent::ToolCallStart {
            context,
            id,
            provider_id,
            name,
            index,
        } => clear_placeholder_then(ConversationIntent::ToolCallStart(ToolCallStart {
            chat_id: crate::tui::model::conversation::ids::ChatId::new(&context.chat_id),
            turn_id: crate::tui::model::conversation::ids::ChatTurnId::new(&context.turn_id),
            id: ToolCallId::new(id),
            provider_id: provider_id.clone(),
            name: name.clone(),
            index: *index,
        })),
        TuiRuntimeEvent::ToolCallUpdate {
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
                .or_else(|| arguments.as_ref().map(ToString::to_string));
            clear_placeholder_then(ConversationIntent::ToolCallUpdate(ToolCallUpdate {
                chat_id: crate::tui::model::conversation::ids::ChatId::new(&context.chat_id),
                turn_id: crate::tui::model::conversation::ids::ChatTurnId::new(&context.turn_id),
                id: ToolCallId::new(id),
                provider_id: provider_id.clone(),
                name: name.clone(),
                index: *index,
                arguments: args
                    .as_ref()
                    .map(|value| sanitize_tool_arguments_delta(name, value)),
                status: match status {
                    TuiToolCallStatus::PendingArgs => ToolCallStatus::PendingArgs,
                    TuiToolCallStatus::Ready => ToolCallStatus::Ready,
                    TuiToolCallStatus::Running => ToolCallStatus::Running,
                },
            }))
        }
        TuiRuntimeEvent::ToolResult {
            context,
            id,
            provider_id,
            tool_name,
            output,
            content,
            is_error,
            images,
        } => clear_placeholder_then(ConversationIntent::ToolResult(ToolResult {
            chat_id: crate::tui::model::conversation::ids::ChatId::new(&context.chat_id),
            turn_id: crate::tui::model::conversation::ids::ChatTurnId::new(&context.turn_id),
            id: ToolCallId::new(id),
            provider_id: provider_id.clone(),
            tool_name: tool_name.clone(),
            output: sanitize_tool_output(tool_name, output),
            content: sanitize_tool_result_content(tool_name, content.clone()),
            is_error: *is_error,
            image_count: images.len(),
        })),
        TuiRuntimeEvent::Usage {
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
        TuiRuntimeEvent::LiveTps(tps) => {
            conversation(ConversationIntent::RecordLiveTps(RecordLiveTps {
                tps: *tps,
            }))
        }
        TuiRuntimeEvent::Error(message) => {
            let mut mapping = conversation(ConversationIntent::AppendError(AppendError {
                text: message.clone(),
            }));
            mapping.diagnostic.push(DiagnosticIntent::RecordNotice {
                severity: DiagnosticSeverity::Error,
                message: message.clone(),
            });
            mapping
        }
        TuiRuntimeEvent::SystemMessage(text) => {
            if strip_system_reminder_envelope(text).trim().is_empty() {
                crate::tui::log_debug!("drop empty runtime system message raw_len={}", text.len());
                AgentEventMapping::default()
            } else {
                conversation(ConversationIntent::AppendSystemMessage(
                    AppendSystemMessage { text: text.clone() },
                ))
            }
        }
        TuiRuntimeEvent::ModelStreamWaiting {
            context,
            elapsed_secs,
            phase,
        } => conversation(ConversationIntent::UpsertModelStreamPlaceholder(
            UpsertModelStreamPlaceholder {
                placeholder: crate::tui::app::event::ModelStreamWaitingView {
                    context: crate::tui::app::event::UiTurnContext {
                        chat_id: crate::tui::model::conversation::ids::ChatId::new(
                            &context.chat_id,
                        ),
                        turn_id: crate::tui::model::conversation::ids::ChatTurnId::new(
                            &context.turn_id,
                        ),
                    },
                    elapsed_secs: *elapsed_secs,
                    phase: phase.clone(),
                },
            },
        )),
        TuiRuntimeEvent::TurnStarted { messages }
        | TuiRuntimeEvent::MicrocompactDone { messages, .. }
        | TuiRuntimeEvent::PostToolExecutionSync { messages }
        | TuiRuntimeEvent::CompactRollback { messages }
        | TuiRuntimeEvent::CompactFinished { messages } => session(SessionIntent::MessagesSynced {
            message_count: messages.len(),
        }),
        TuiRuntimeEvent::StopHookBlocked { messages } => {
            let mut mapping = session(SessionIntent::MessagesSynced {
                message_count: messages.len(),
            });
            if let Some(message) = messages
                .iter()
                .rev()
                .find(|message| message.source == TuiMessageSource::StopHook)
            {
                mapping
                    .conversation
                    .push(ConversationIntent::AppendHookNotice(AppendHookNotice {
                        content: stop_hook_notice_content(message),
                    }));
            }
            mapping
        }
        TuiRuntimeEvent::ApiError { messages, error } => {
            let mut mapping = session(SessionIntent::MessagesSynced {
                message_count: messages.len(),
            });
            mapping
                .conversation
                .push(ConversationIntent::AppendError(AppendError {
                    text: error.clone(),
                }));
            mapping
        }
        TuiRuntimeEvent::UserMessagesAdopted { queued, .. }
        | TuiRuntimeEvent::UserMessagesQueued { queued } => conversation(
            ConversationIntent::SyncQueuedSubmissions(SyncQueuedSubmissions {
                queued: queued.clone(),
            }),
        ),
        TuiRuntimeEvent::Done { context, .. } | TuiRuntimeEvent::Cancelled { context } => {
            conversation(ConversationIntent::CompleteChat(CompleteChat {
                chat_id: crate::tui::model::conversation::ids::ChatId::new(&context.chat_id),
                turn_id: crate::tui::model::conversation::ids::ChatTurnId::new(&context.turn_id),
            }))
        }
        TuiRuntimeEvent::GraphPhaseChanged { node, .. } => {
            conversation(ConversationIntent::SetGraphPhase(SetGraphPhase(
                (node != "idle").then(|| node.clone()),
            )))
        }
        TuiRuntimeEvent::CompactProgress {
            stage,
            current,
            total,
        } => conversation(ConversationIntent::SetCompactProgress(SetCompactProgress {
            stage: stage.clone(),
            current: *current,
            total: *total,
        })),
        TuiRuntimeEvent::TasksSnapshot { lines } => conversation(
            ConversationIntent::UpdateTaskLines(UpdateTaskLines(lines.clone())),
        ),
        TuiRuntimeEvent::SessionReset => AgentEventMapping::default(),
        TuiRuntimeEvent::UserMessagesWithdrawn { texts: _ } => conversation(
            ConversationIntent::ClearAllQueuedSubmissions(ClearAllQueuedSubmissions),
        ),
        TuiRuntimeEvent::ModelInvocationRetrying {
            attempt, delay_ms, ..
        } => conversation(ConversationIntent::AppendSystemMessage(
            AppendSystemMessage {
                text: format!(
                    "Retrying model invocation (attempt {attempt}) in {:.1}s.",
                    *delay_ms as f64 / 1_000.0
                ),
            },
        )),
        TuiRuntimeEvent::TurnChanged(_) => AgentEventMapping::default(),
        TuiRuntimeEvent::HookEvent(event) => {
            if event.hook_name == "PostCompact"
                || (event.hook_name == "Stop" && event.status == TuiHookStatus::Blocked)
            {
                AgentEventMapping::default()
            } else if let Some(notice) = crate::tui::adapter::hook_notice::hook_event_notice(event)
            {
                conversation(ConversationIntent::AppendHookNotice(AppendHookNotice {
                    content: notice,
                }))
            } else {
                AgentEventMapping::default()
            }
        }
        TuiRuntimeEvent::HookMessage(message) => {
            if message.point == "Stop" {
                AgentEventMapping::default()
            } else if let Some(notice) =
                crate::tui::adapter::hook_notice::hook_message_notice(message)
            {
                conversation(ConversationIntent::AppendHookNotice(AppendHookNotice {
                    content: notice,
                }))
            } else {
                AgentEventMapping::default()
            }
        }
        TuiRuntimeEvent::AgentProgress {
            context,
            tool_id,
            event,
        } => match &event.kind {
            TuiAgentProgressKind::Started { role, model } => {
                conversation(ConversationIntent::UpdateAgentMeta(UpdateAgentMeta {
                    chat_id: crate::tui::model::conversation::ids::ChatId::new(&context.chat_id),
                    turn_id: crate::tui::model::conversation::ids::ChatTurnId::new(
                        &context.turn_id,
                    ),
                    tool_id: ToolCallId::new(tool_id),
                    role: role.clone(),
                    model: model.clone(),
                }))
            }
            TuiAgentProgressKind::ToolOutput { .. } => AgentEventMapping::default(),
            TuiAgentProgressKind::Message { text } => conversation(
                ConversationIntent::RecordAgentProgress(RecordAgentProgress {
                    chat_id: crate::tui::model::conversation::ids::ChatId::new(&context.chat_id),
                    turn_id: crate::tui::model::conversation::ids::ChatTurnId::new(
                        &context.turn_id,
                    ),
                    tool_id: ToolCallId::new(tool_id),
                    message: format_agent_progress_text(text),
                }),
            ),
            TuiAgentProgressKind::ToolCalls { calls } => conversation(
                ConversationIntent::RecordAgentProgress(RecordAgentProgress {
                    chat_id: crate::tui::model::conversation::ids::ChatId::new(&context.chat_id),
                    turn_id: crate::tui::model::conversation::ids::ChatTurnId::new(
                        &context.turn_id,
                    ),
                    tool_id: ToolCallId::new(tool_id),
                    message: format_agent_progress_calls(calls),
                }),
            ),
        },
        TuiRuntimeEvent::ConfigChanged { .. } => AgentEventMapping::default(),
        TuiRuntimeEvent::ConfigReloaded { changed_keys } => conversation(
            ConversationIntent::AppendSystemMessage(AppendSystemMessage {
                text: format!("[config reloaded] changed: {}", changed_keys.join(", ")),
            }),
        ),
        TuiRuntimeEvent::ModelSwitched { .. } => AgentEventMapping::default(),
        TuiRuntimeEvent::ThinkingChanged { .. } => AgentEventMapping::default(),
        TuiRuntimeEvent::ContextEstimated { .. } => AgentEventMapping::default(),
        TuiRuntimeEvent::CommandResultText { text, is_error } => {
            if *is_error {
                conversation(ConversationIntent::AppendError(AppendError {
                    text: text.clone(),
                }))
            } else {
                conversation(ConversationIntent::AppendSystemMessage(
                    AppendSystemMessage { text: text.clone() },
                ))
            }
        }
        TuiRuntimeEvent::SessionResumed { .. } => AgentEventMapping::default(),
        TuiRuntimeEvent::SessionResumeFailed { message, .. } => {
            conversation(ConversationIntent::AppendError(AppendError {
                text: message.clone(),
            }))
        }
        TuiRuntimeEvent::ReflectionHistory { .. } => AgentEventMapping::default(),
        TuiRuntimeEvent::ModelList { .. } => AgentEventMapping::default(),
        TuiRuntimeEvent::ReminderList { .. } => AgentEventMapping::default(),
        TuiRuntimeEvent::SessionList { .. } => AgentEventMapping::default(),
        TuiRuntimeEvent::ProjectInfo { .. } => AgentEventMapping::default(),
        TuiRuntimeEvent::CostUpdate { .. } => AgentEventMapping::default(),
        TuiRuntimeEvent::Run { run_id, event, .. } => match event {
            TuiRunEvent::Started => conversation(ConversationIntent::RunStarted(RunStarted {
                run_id: run_id.clone(),
            })),
            TuiRunEvent::AwaitingUser => {
                conversation(ConversationIntent::RunAwaitingUser(RunAwaitingUser {
                    run_id: run_id.clone(),
                }))
            }
            TuiRunEvent::Resumed => conversation(ConversationIntent::RunResumed(RunResumed {
                run_id: run_id.clone(),
            })),
            TuiRunEvent::Cancelling => {
                conversation(ConversationIntent::RunCancelling(RunCancelling {
                    run_id: run_id.clone(),
                }))
            }
            TuiRunEvent::Cancelled => {
                conversation(ConversationIntent::RunCancelled(RunCancelled {
                    run_id: run_id.clone(),
                }))
            }
            TuiRunEvent::Completed { .. } => {
                conversation(ConversationIntent::RunCompleted(RunCompleted {
                    run_id: run_id.clone(),
                }))
            }
            TuiRunEvent::Failed { .. } => conversation(ConversationIntent::RunFailed(RunFailed {
                run_id: run_id.clone(),
            })),
            TuiRunEvent::Stuck { reason } => {
                conversation(ConversationIntent::AppendError(AppendError {
                    text: reason.clone(),
                }))
            }
            TuiRunEvent::DrainingInput
            | TuiRunEvent::TerminationRequested { .. }
            | TuiRunEvent::Terminated { .. }
            | TuiRunEvent::Transitioned { .. } => AgentEventMapping::default(),
        },
        TuiRuntimeEvent::RunStep {
            run_id,
            step_id,
            event,
            ..
        } => match event {
            TuiRunStepEvent::Started => {
                conversation(ConversationIntent::RunStepStarted(RunStepStarted {
                    run_id: run_id.clone(),
                    step_id: step_id.clone(),
                    tool_reference: None,
                }))
            }
            TuiRunStepEvent::Completed => {
                conversation(ConversationIntent::RunStepCompleted(RunStepCompleted {
                    run_id: run_id.clone(),
                    step_id: step_id.clone(),
                }))
            }
            TuiRunStepEvent::CancellationRequested
            | TuiRunStepEvent::FinalizationStarted
            | TuiRunStepEvent::Cancelled { .. } => AgentEventMapping::default(),
        },
        TuiRuntimeEvent::InteractionRequested(request) => {
            let body = match &request.body {
                TuiInteractionBody::UserQuestions(questions) => InteractionBody::UserQuestions(
                    questions
                        .iter()
                        .map(|question| UiUserQuestion {
                            prompt: question.prompt.clone(),
                            options: question.options.clone(),
                            allow_multi: question.allow_multi,
                        })
                        .collect(),
                ),
                TuiInteractionBody::ToolApproval(prompt) => {
                    InteractionBody::ToolApproval(UiApprovalPrompt {
                        title: prompt.tool_name.clone(),
                        detail: prompt.args_summary.clone(),
                        risk: match prompt.risk_level {
                            crate::tui::adapter::tui_runtime_event::TuiRiskLevel::Low => {
                                UiRiskLevel::Low
                            }
                            crate::tui::adapter::tui_runtime_event::TuiRiskLevel::Medium => {
                                UiRiskLevel::Medium
                            }
                            crate::tui::adapter::tui_runtime_event::TuiRiskLevel::High => {
                                UiRiskLevel::High
                            }
                        },
                    })
                }
                TuiInteractionBody::PlanApproval(prompt) => {
                    InteractionBody::PlanApproval(UiPlanApprovalPrompt {
                        title: prompt.plan_title.clone(),
                        steps: prompt.steps.clone(),
                    })
                }
                TuiInteractionBody::HardPause(diagnostic) => {
                    InteractionBody::HardPause(UiStuckDiagnostic {
                        reason: diagnostic.reason.clone(),
                        recent_actions: diagnostic.recent_actions.clone(),
                    })
                }
            };
            log::info!(
                target: crate::LOG_TARGET,
                "[interaction] map_runtime_event → ShowInteraction request_id={:?} run_id={:?}",
                request.request_id, request.run_id,
            );
            conversation(ConversationIntent::ShowInteraction(ShowInteraction {
                request: InteractionRequest {
                    request_id: request.request_id.clone(),
                    run_id: request.run_id.clone(),
                    body,
                },
            }))
        }
        TuiRuntimeEvent::WorkspaceSnapshot(snapshot) => AgentEventMapping {
            workspace: vec![WorkspaceIntent::ApplySnapshot {
                path_base: Some(snapshot.path_base.clone()),
                workspace_root: Some(snapshot.workspace_root.clone()),
            }],
            ..AgentEventMapping::default()
        },
    }
}

fn format_agent_progress_text(text: &str) -> String {
    if text.is_empty() || text.ends_with('\n') {
        text.to_string()
    } else {
        format!("{text}\n")
    }
}

fn format_agent_progress_calls(
    calls: &[crate::tui::adapter::tui_runtime_event::TuiAgentToolCall],
) -> String {
    let text = calls
        .iter()
        .map(|call| {
            let name = crate::tui::view_model::tool_name::tool_display_name(&call.name);
            let raw = match &call.input {
                serde_json::Value::String(s) => s.clone(),
                value => value.to_string(),
            };
            let preview = truncate_json(&raw);
            if preview.is_empty() {
                format!("→ {name}")
            } else {
                format!("→ {name} {preview}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    format_agent_progress_text(&text)
}

/// Truncate a JSON string to at most 120 chars for preview. Modeled after
/// the same logic in `render::output::tool_display::format`  to avoid a
/// render → adapter dependency.
fn truncate_json(raw: &str) -> &str {
    let raw = raw.trim();
    if raw.len() <= 120 {
        raw
    } else {
        &raw[..120]
    }
}

fn map_status_context(update: &StatusContextUpdate) -> AgentEventMapping {
    AgentEventMapping {
        workspace: vec![WorkspaceIntent::ApplySnapshot {
            path_base: Some(update.raw_path_base.to_string_lossy().to_string()),
            workspace_root: Some(update.raw_workspace_root.to_string_lossy().to_string()),
        }],
        ..AgentEventMapping::default()
    }
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
