use crate::tui::adapter::tui_runtime_event::{TuiRuntimeEvent, TuiToolCallStatus};
use crate::tui::model::conversation::ids::ToolCallId;
use crate::tui::model::conversation::intent::*;
use crate::tui::model::conversation::system_reminder::strip_system_reminder_envelope;
use crate::tui::model::conversation::tool_call::ToolCallStatus;
use crate::tui::model::diagnostic::intent::DiagnosticIntent;
use crate::tui::model::diagnostic::notice::DiagnosticSeverity;
use crate::tui::model::runtime::session_intent::SessionIntent;
use crate::tui::model::workspace_provider::WorkspaceIntent;

mod sanitize;

#[cfg(test)]
#[path = "agent_event_runtime_tests.rs"]
mod runtime_tests;

use sanitize::{sanitize_tool_arguments_delta, sanitize_tool_output, sanitize_tool_result_content};

#[derive(Debug, Default, PartialEq)]
pub struct AgentEventMapping {
    pub conversation: Vec<ConversationIntent>,
    pub diagnostic: Vec<DiagnosticIntent>,
    pub session: Vec<SessionIntent>,
    pub workspace: Vec<WorkspaceIntent>,
    pub ui_preferences: Vec<crate::tui::model::ui_preferences::UiPreferencesIntent>,
}

pub fn map_runtime_event(event: &TuiRuntimeEvent) -> AgentEventMapping {
    use crate::tui::adapter::tui_runtime_event::{TuiInteractionBody, TuiRunStepEvent};
    use crate::tui::model::conversation::interaction::{
        InteractionBody, InteractionRequest, UiApprovalPrompt, UiPlanApprovalPrompt, UiRiskLevel,
        UiStuckDiagnostic, UiUserQuestion,
    };

    match event {
        TuiRuntimeEvent::Noop => AgentEventMapping::default(),
        TuiRuntimeEvent::ActivityChanged { kind, activity } => conversation(
            ConversationIntent::ObserveActivityChange(ObserveActivityChange {
                kind: *kind,
                activity: activity.clone(),
            }),
        ),
        TuiRuntimeEvent::ActivitySnapshot(snapshot) => conversation(
            ConversationIntent::ReplaceActivitySnapshot(ReplaceActivitySnapshot {
                snapshot: snapshot.clone(),
            }),
        ),
        TuiRuntimeEvent::SkillsUpdated { .. } => AgentEventMapping::default(),
        TuiRuntimeEvent::AssistantTextDelta { context, delta } => {
            conversation(ConversationIntent::AssistantText(AssistantText {
                chat_id: crate::tui::model::conversation::ids::ChatId::new(&context.chat_id),
                run_id: crate::tui::model::conversation::ids::ChatRunId::new(&context.run_id),
                text: delta.clone(),
            }))
        }
        TuiRuntimeEvent::ThinkingDelta { context, delta } => {
            conversation(ConversationIntent::ThinkingText(ThinkingText {
                chat_id: crate::tui::model::conversation::ids::ChatId::new(&context.chat_id),
                run_id: crate::tui::model::conversation::ids::ChatRunId::new(&context.run_id),
                text: delta.clone(),
            }))
        }
        TuiRuntimeEvent::BlockComplete { context, .. } => {
            conversation(ConversationIntent::CompleteBlock(CompleteBlock {
                chat_id: crate::tui::model::conversation::ids::ChatId::new(&context.chat_id),
                run_id: crate::tui::model::conversation::ids::ChatRunId::new(&context.run_id),
            }))
        }
        TuiRuntimeEvent::ToolCallStarted {
            context,
            id,
            provider_id,
            name,
            index,
        } => conversation(ConversationIntent::ToolCallStart(ToolCallStart {
            chat_id: crate::tui::model::conversation::ids::ChatId::new(&context.chat_id),
            run_id: crate::tui::model::conversation::ids::ChatRunId::new(&context.run_id),
            id: ToolCallId::new(id),
            provider_id: provider_id.clone(),
            name: name.clone(),
            index: *index,
        })),
        TuiRuntimeEvent::ToolCallArgumentsDelta {
            context,
            id,
            provider_id,
            name,
            index,
            delta,
        } => conversation(ConversationIntent::ToolCallUpdate(ToolCallUpdate {
            chat_id: crate::tui::model::conversation::ids::ChatId::new(&context.chat_id),
            run_id: crate::tui::model::conversation::ids::ChatRunId::new(&context.run_id),
            id: ToolCallId::new(id),
            provider_id: provider_id.clone(),
            name: name.clone(),
            index: *index,
            arguments: Some(sanitize_tool_arguments_delta(name, delta)),
            status: ToolCallStatus::PendingArgs,
        })),
        TuiRuntimeEvent::ToolCallStateChanged {
            context,
            id,
            provider_id,
            name,
            index,
            arguments,
            status,
        } => {
            let args = arguments.as_ref().map(ToString::to_string);
            conversation(ConversationIntent::ToolCallUpdate(ToolCallUpdate {
                chat_id: crate::tui::model::conversation::ids::ChatId::new(&context.chat_id),
                run_id: crate::tui::model::conversation::ids::ChatRunId::new(&context.run_id),
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
        } => conversation(ConversationIntent::ToolResult(ToolResult {
            chat_id: crate::tui::model::conversation::ids::ChatId::new(&context.chat_id),
            run_id: crate::tui::model::conversation::ids::ChatRunId::new(&context.run_id),
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
        TuiRuntimeEvent::HookNotice(notice) => {
            conversation(ConversationIntent::AppendHookNotice(AppendHookNotice {
                title: notice.title(),
                text: notice.display_text(),
                kind: notice.kind.clone(),
            }))
        }
        TuiRuntimeEvent::TurnStarted { messages }
        | TuiRuntimeEvent::MicrocompactCompleted { messages, .. }
        | TuiRuntimeEvent::CompactOperationRolledBack { messages } => {
            session(SessionIntent::MessagesSynced {
                message_count: messages.len(),
            })
        }
        TuiRuntimeEvent::SessionMessageStateChanged {
            message_count,
            revision,
        } => session(SessionIntent::MessageStateChanged {
            message_count: *message_count,
            revision: *revision,
        }),
        TuiRuntimeEvent::CompactOperationCompleted { messages, notice } => {
            let mut mapping = conversation(ConversationIntent::AppendSystemMessage(
                AppendSystemMessage {
                    text: notice.clone(),
                },
            ));
            mapping.session.push(SessionIntent::MessagesSynced {
                message_count: messages.len(),
            });
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
        TuiRuntimeEvent::Done {
            context,
            duration_ms,
        } => AgentEventMapping {
            conversation: vec![
                ConversationIntent::CompleteChat(CompleteChat {
                    chat_id: crate::tui::model::conversation::ids::ChatId::new(&context.chat_id),
                    run_id: crate::tui::model::conversation::ids::ChatRunId::new(&context.run_id),
                }),
                ConversationIntent::TerminalNotice(TerminalNotice {
                    cause: crate::tui::model::conversation::terminal::TerminalCause::Completed,
                    duration: duration_ms.map(std::time::Duration::from_millis),
                }),
            ],
            ..AgentEventMapping::default()
        },
        TuiRuntimeEvent::Cancelled {
            context,
            duration_ms,
        } => AgentEventMapping {
            conversation: vec![
                ConversationIntent::CompleteChat(CompleteChat {
                    chat_id: crate::tui::model::conversation::ids::ChatId::new(&context.chat_id),
                    run_id: crate::tui::model::conversation::ids::ChatRunId::new(&context.run_id),
                }),
                ConversationIntent::TerminalNotice(TerminalNotice {
                    cause: crate::tui::model::conversation::terminal::TerminalCause::UserCancelled,
                    duration: Some(std::time::Duration::from_millis(*duration_ms)),
                }),
            ],
            ..AgentEventMapping::default()
        },
        TuiRuntimeEvent::GraphPhaseChanged { node, .. } => {
            conversation(ConversationIntent::SetGraphPhase(SetGraphPhase(
                (node != "idle").then(|| node.clone()),
            )))
        }
        TuiRuntimeEvent::RuntimeStatusChanged { status } => conversation(
            ConversationIntent::ReplaceRuntimeStatus(ReplaceRuntimeStatus((**status).clone())),
        ),
        TuiRuntimeEvent::TaskStateChanged { state } => conversation(
            ConversationIntent::ReplaceTaskState(ReplaceTaskState((**state).clone())),
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
        TuiRuntimeEvent::RunChanged(_) => AgentEventMapping::default(),
        TuiRuntimeEvent::ToolOutputDelta {
            context,
            tool_id,
            delta,
        } => conversation(ConversationIntent::RecordToolStreamingOutput(
            RecordToolStreamingOutput {
                chat_id: crate::tui::model::conversation::ids::ChatId::new(&context.chat_id),
                run_id: crate::tui::model::conversation::ids::ChatRunId::new(&context.run_id),
                tool_id: ToolCallId::new(tool_id),
                text: delta.clone(),
            },
        )),
        TuiRuntimeEvent::SubRunStarted(event) => {
            conversation(ConversationIntent::UpdateAgentMeta(UpdateAgentMeta {
                chat_id: crate::tui::model::conversation::ids::ChatId::new(
                    &event.identity.parent_chat_id,
                ),
                run_id: crate::tui::model::conversation::ids::ChatRunId::new(
                    event.identity.parent_run_id.as_str(),
                ),
                tool_id: ToolCallId::new(&event.identity.spawned_by_tool_call_id),
                role: event.role.clone(),
                model: event.model.clone(),
            }))
        }
        TuiRuntimeEvent::SubRunActivity(event) => conversation(
            ConversationIntent::RecordSubRunActivity(RecordSubRunActivity {
                agent_id: event.identity.agent_id.clone(),
                sub_run_id: event.identity.run_id.as_str().to_string(),
                parent_run_id: event.identity.parent_run_id.as_str().to_string(),
                spawned_by_tool_call_id: ToolCallId::new(&event.identity.spawned_by_tool_call_id),
                sequence: event.sequence,
                sequence_index: event.sequence_index,
                kind: event.kind.clone(),
            }),
        ),
        TuiRuntimeEvent::ConfigChanged { view, .. } => AgentEventMapping {
            ui_preferences: vec![
                crate::tui::model::ui_preferences::UiPreferencesIntent::MarkdownSpacingChanged(
                    view.markdown_spacing,
                ),
            ],
            ..Default::default()
        },
        TuiRuntimeEvent::ConfigReloaded { changed_keys, view } => {
            let mut mapping = conversation(ConversationIntent::AppendSystemMessage(
                AppendSystemMessage {
                    text: format!("[config reloaded] changed: {}", changed_keys.join(", ")),
                },
            ));
            mapping.ui_preferences.push(
                crate::tui::model::ui_preferences::UiPreferencesIntent::MarkdownSpacingChanged(
                    view.markdown_spacing,
                ),
            );
            mapping
        }
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
        TuiRuntimeEvent::Run { .. } => AgentEventMapping::default(),
        TuiRuntimeEvent::RunStep {
            run_id,
            parent_run_id,
            step_id,
            event,
        } => match event {
            TuiRunStepEvent::Cancelled { terminal } if parent_run_id.is_none() => {
                let confirmed = matches!(
                    terminal,
                    crate::tui::adapter::tui_runtime_event::TuiRunStepCancellationTerminal::Cancelled
                );
                crate::tui::log_debug!(
                    "cancelled step terminal consumed: run_id={:?} step_id={:?} confirmed={}",
                    run_id,
                    step_id,
                    confirmed
                );
                conversation(ConversationIntent::PresentCancelledStep(
                    PresentCancelledStep { confirmed },
                ))
            }
            TuiRunStepEvent::Started
            | TuiRunStepEvent::Completed
            | TuiRunStepEvent::CancellationRequested
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
                    tool_call_id: request.tool_call_id.clone(),
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

// ════════════════════════════════════════════════════════════════════
//  Helpers — AgentEventMapping constructors
// ════════════════════════════════════════════════════════════════════

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
