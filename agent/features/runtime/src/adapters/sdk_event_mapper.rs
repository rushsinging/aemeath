//! Runtime-owned mappers to the SDK Published Language.

use crate::application::loop_engine::chat::RuntimeRunContext;
use crate::domain::agent_run::RuntimeLifecycleEvent;
use sdk::{
    AgentProgressEventView, AgentProgressKindView, AgentToolCallProgressView, ChatEvent,
    ChatEventContext, ChildRunActivityEventView, ChildRunActivityKindView, ChildRunIdentityView,
    ChildRunTerminalOutcomeView, RunStatusView, ToolCallStatusView, ToolResultImage,
};

pub fn map_lifecycle_event(event: RuntimeLifecycleEvent) -> ChatEvent {
    match event {
        RuntimeLifecycleEvent::Started {
            run_id,
            parent_run_id,
        } => ChatEvent::RunStarted {
            run_id,
            parent_run_id,
        },
        RuntimeLifecycleEvent::StepStarted {
            run_id,
            parent_run_id,
            step_id,
        } => ChatEvent::RunStepStarted {
            run_id,
            parent_run_id,
            step_id,
        },
        RuntimeLifecycleEvent::StepCompleted {
            run_id,
            parent_run_id,
            step_id,
        } => ChatEvent::RunStepCompleted {
            run_id,
            parent_run_id,
            step_id,
        },
        RuntimeLifecycleEvent::StepCancellationRequested {
            run_id,
            parent_run_id,
            step_id,
        } => ChatEvent::RunStepCancellationRequested {
            run_id,
            parent_run_id,
            step_id,
        },
        RuntimeLifecycleEvent::StepFinalizationStarted {
            run_id,
            parent_run_id,
            step_id,
        } => ChatEvent::RunStepFinalizationStarted {
            run_id,
            parent_run_id,
            step_id,
        },
        RuntimeLifecycleEvent::StepCancelled {
            run_id,
            parent_run_id,
            step_id,
            terminal,
        } => ChatEvent::RunStepCancelled {
            run_id,
            parent_run_id,
            step_id,
            terminal,
        },
        RuntimeLifecycleEvent::DrainingInput {
            run_id,
            parent_run_id,
        } => ChatEvent::RunDrainingInput {
            run_id,
            parent_run_id,
        },
        RuntimeLifecycleEvent::TerminationRequested {
            run_id,
            parent_run_id,
            reason,
            deadline,
        } => ChatEvent::RunTerminationRequested {
            run_id,
            parent_run_id,
            reason,
            deadline,
        },
        RuntimeLifecycleEvent::Terminated {
            run_id,
            parent_run_id,
            reason,
        } => ChatEvent::RunTerminated {
            run_id,
            parent_run_id,
            reason,
        },
        RuntimeLifecycleEvent::Completed {
            run_id,
            parent_run_id,
            result,
            ..
        } => ChatEvent::RunCompleted {
            run_id,
            parent_run_id,
            result,
        },
        RuntimeLifecycleEvent::Failed {
            run_id,
            parent_run_id,
            error,
        } => ChatEvent::RunFailed {
            run_id,
            parent_run_id,
            error,
        },
        RuntimeLifecycleEvent::StuckDetected {
            run_id,
            parent_run_id,
            reason,
        } => ChatEvent::RunStuckDetected {
            run_id,
            parent_run_id,
            reason,
        },
        RuntimeLifecycleEvent::Transitioned {
            run_id,
            parent_run_id,
            to,
            timing,
            ..
        } => ChatEvent::RunTransitioned {
            run_id,
            parent_run_id,
            status: run_status_to_sdk(to),
            timing: sdk::RunTimingView {
                observation_revision: timing.observation_revision,
                total_elapsed_ms: timing.total_elapsed_ms,
                phase_elapsed_ms: timing.phase_elapsed_ms,
            },
        },
        RuntimeLifecycleEvent::AwaitingUser {
            run_id,
            parent_run_id,
            ..
        } => ChatEvent::RunAwaitingUser {
            run_id,
            parent_run_id,
        },
        RuntimeLifecycleEvent::Resumed {
            run_id,
            parent_run_id,
            ..
        } => ChatEvent::RunResumed {
            run_id,
            parent_run_id,
        },
    }
}

fn run_status_to_sdk(status: crate::domain::agent_run::RunStatus) -> RunStatusView {
    use crate::domain::agent_run::RunStatus;

    match status {
        RunStatus::Created => RunStatusView::Created,
        RunStatus::DrainingInput => RunStatusView::DrainingInput,
        RunStatus::PreparingContext => RunStatusView::PreparingContext,
        RunStatus::InvokingModel => RunStatusView::InvokingModel,
        RunStatus::ApplyingResponse => RunStatusView::ApplyingResponse,
        RunStatus::AwaitingToolApproval => RunStatusView::AwaitingToolApproval,
        RunStatus::ExecutingTools => RunStatusView::ExecutingTools,
        RunStatus::AwaitingUser => RunStatusView::AwaitingUser,
        RunStatus::Compacting => RunStatusView::Compacting,
        RunStatus::CancellingStep => RunStatusView::CancellingStep,
        RunStatus::FinalizingStep => RunStatusView::FinalizingStep,
        RunStatus::Terminating => RunStatusView::Terminating,
        RunStatus::Completed => RunStatusView::Completed,
        RunStatus::Failed => RunStatusView::Failed,
        RunStatus::Terminated => RunStatusView::Terminated,
    }
}

fn turn_context_to_sdk(context: RuntimeRunContext) -> ChatEventContext {
    ChatEventContext::new(context.chat_id, context.run_id)
}

fn tool_call_status_to_sdk(
    status: crate::application::loop_engine::chat::RuntimeToolCallStatus,
) -> ToolCallStatusView {
    match status {
        crate::application::loop_engine::chat::RuntimeToolCallStatus::PendingArgs => {
            ToolCallStatusView::PendingArgs
        }
        crate::application::loop_engine::chat::RuntimeToolCallStatus::Ready => {
            ToolCallStatusView::Ready
        }
        crate::application::loop_engine::chat::RuntimeToolCallStatus::Running => {
            ToolCallStatusView::Running
        }
    }
}

pub(crate) fn map_display_history_index(
    index: context::api::DisplayHistoryStepIndex,
) -> sdk::DisplayHistoryIndex {
    sdk::DisplayHistoryIndex {
        session_id: index.session_id().to_string(),
        generation_revision: index.generation_revision(),
        steps: index
            .steps()
            .iter()
            .map(|step| sdk::DisplayHistoryStepReference {
                run_id: step.run_id().to_string(),
                step_id: step.step_id().to_string(),
                member_name: step.member_name().to_string(),
                estimated_lines: step.estimated_lines(),
                user_input_history: step.user_input_history().to_vec(),
                finalize_cause: step
                    .finalize_cause()
                    .map(crate::application::client::map_finalize_cause_to_sdk),
                duration_ms: step.duration_ms(),
            })
            .collect(),
    }
}

pub(crate) fn map_activity_event(
    event: crate::application::loop_engine::chat::RuntimeActivityEvent,
) -> ChatEvent {
    match event {
        crate::application::loop_engine::chat::RuntimeActivityEvent::Changed { kind, activity } => {
            ChatEvent::ActivityChanged {
                kind,
                activity: *activity,
            }
        }
        crate::application::loop_engine::chat::RuntimeActivityEvent::Snapshot(snapshot) => {
            ChatEvent::ActivitySnapshot(snapshot)
        }
    }
}

pub(crate) fn map_stream_event(
    event: crate::application::loop_engine::chat::RuntimeStreamEvent,
) -> ChatEvent {
    match event {
        crate::application::loop_engine::chat::RuntimeStreamEvent::AssistantTextDelta {
            context,
            delta,
        } => ChatEvent::AssistantTextDelta {
            context: turn_context_to_sdk(context),
            delta,
        },
        crate::application::loop_engine::chat::RuntimeStreamEvent::ThinkingDelta {
            context,
            delta,
        } => ChatEvent::ThinkingDelta {
            context: turn_context_to_sdk(context),
            delta,
        },
        crate::application::loop_engine::chat::RuntimeStreamEvent::BlockComplete {
            context,
            text,
        } => ChatEvent::BlockComplete {
            context: turn_context_to_sdk(context),
            text,
        },
        crate::application::loop_engine::chat::RuntimeStreamEvent::ToolCallStarted {
            context,
            id,
            provider_id,
            name,
            index,
        } => ChatEvent::ToolCallStarted {
            context: turn_context_to_sdk(context),
            id,
            provider_id,
            name,
            index,
        },
        crate::application::loop_engine::chat::RuntimeStreamEvent::ToolCallArgumentsDelta {
            context,
            id,
            provider_id,
            name,
            index,
            delta,
        } => ChatEvent::ToolCallArgumentsDelta {
            context: turn_context_to_sdk(context),
            id,
            provider_id,
            name,
            index,
            delta,
        },
        crate::application::loop_engine::chat::RuntimeStreamEvent::ToolCallStateChanged {
            context,
            id,
            provider_id,
            name,
            index,
            arguments,
            status,
        } => ChatEvent::ToolCallStateChanged {
            context: turn_context_to_sdk(context),
            id,
            provider_id,
            name,
            index,
            arguments,
            status: tool_call_status_to_sdk(status),
        },
        crate::application::loop_engine::chat::RuntimeStreamEvent::ToolResult {
            context,
            id,
            provider_id,
            tool_name,
            output,
            content,
            is_error,
            images,
        } => ChatEvent::ToolResult {
            context: turn_context_to_sdk(context),
            id,
            provider_id,
            tool_name,
            output,
            content,
            is_error,
            images: images
                .into_iter()
                .map(|image| ToolResultImage {
                    base64: image.base64,
                    media_type: image.media_type,
                })
                .collect(),
        },
        crate::application::loop_engine::chat::RuntimeStreamEvent::SystemMessage(msg) => {
            ChatEvent::SystemMessage(msg)
        }
        crate::application::loop_engine::chat::RuntimeStreamEvent::ModelInvocationRetrying {
            context,
            attempt,
            delay,
        } => ChatEvent::ModelInvocationRetrying {
            context: turn_context_to_sdk(context),
            attempt,
            delay,
        },
        crate::application::loop_engine::chat::RuntimeStreamEvent::Usage {
            input,
            output,
            last_input,
            elapsed_secs,
        } => ChatEvent::Usage {
            input,
            output,
            last_input,
            elapsed_secs,
        },
        crate::application::loop_engine::chat::RuntimeStreamEvent::TurnStarted { messages } => {
            ChatEvent::TurnStarted {
                messages: messages
                    .into_iter()
                    .map(crate::application::client::message_to_sdk)
                    .collect(),
            }
        }
        crate::application::loop_engine::chat::RuntimeStreamEvent::MicrocompactCompleted {
            messages,
            cleared_count,
        } => ChatEvent::MicrocompactCompleted {
            messages: messages
                .into_iter()
                .map(crate::application::client::message_to_sdk)
                .collect(),
            cleared_count,
        },
        crate::application::loop_engine::chat::RuntimeStreamEvent::SessionMessageStateChanged {
            message_count,
            revision,
        } => ChatEvent::SessionMessageStateChanged {
            message_count,
            revision,
        },
        crate::application::loop_engine::chat::RuntimeStreamEvent::HookNotice(notice) => {
            ChatEvent::HookNotice {
                notice: sdk::HookNoticeView {
                    point: notice.point,
                    kind: match notice.kind {
                        share::message::HookNoticeKind::Blocked => sdk::HookNoticeKindView::Blocked,
                        share::message::HookNoticeKind::Failed => sdk::HookNoticeKindView::Failed,
                        share::message::HookNoticeKind::Info => sdk::HookNoticeKindView::Info,
                    },
                    summary: notice.summary,
                    command: notice.command,
                    exit_code: notice.exit_code,
                    reason: notice.reason,
                    stdout_preview: notice.stdout_preview,
                    stderr_preview: notice.stderr_preview,
                    stdout_truncated: notice.stdout_truncated,
                    stderr_truncated: notice.stderr_truncated,
                    output_file: notice.output_file,
                },
            }
        }
        crate::application::loop_engine::chat::RuntimeStreamEvent::ApiError { messages, error } => {
            ChatEvent::ApiError {
                messages: messages
                    .into_iter()
                    .map(crate::application::client::message_to_sdk)
                    .collect(),
                error,
            }
        }
        crate::application::loop_engine::chat::RuntimeStreamEvent::CompactOperationRolledBack {
            messages,
        } => ChatEvent::CompactOperationRolledBack {
            messages: messages
                .into_iter()
                .map(crate::application::client::message_to_sdk)
                .collect(),
        },
        crate::application::loop_engine::chat::RuntimeStreamEvent::CompactOperationCompleted {
            messages,
            notice,
        } => ChatEvent::CompactOperationCompleted {
            messages: messages
                .into_iter()
                .map(crate::application::client::message_to_sdk)
                .collect(),
            notice,
        },
        crate::application::loop_engine::chat::RuntimeStreamEvent::UserMessagesAdopted {
            items,
            queued,
        } => {
            let skill_items = items
                .iter()
                .filter(|(_, message)| {
                    message.metadata.as_ref().is_some_and(|metadata| {
                        matches!(metadata.source, share::message::MessageSource::SkillRequest)
                    })
                })
                .count();
            log::debug!(
                target: crate::LOG_TARGET,
                "skill_request boundary=runtime_to_sdk_adopted items={} queued={} skill_items={}",
                items.len(),
                queued.len(),
                skill_items
            );
            ChatEvent::UserMessagesAdopted {
                items: items
                    .into_iter()
                    .map(|(id, message)| {
                        let mut value = crate::application::client::message_to_sdk(message);
                        value.input_id = Some(id);
                        value
                    })
                    .collect(),
                queued: queued
                    .into_iter()
                    .map(|(id, message)| {
                        let mut value = crate::application::client::message_to_sdk(message);
                        value.input_id = Some(id);
                        value
                    })
                    .collect(),
            }
        }
        crate::application::loop_engine::chat::RuntimeStreamEvent::UserMessagesQueued {
            queued,
        } => ChatEvent::UserMessagesQueued {
            queued: queued
                .into_iter()
                .map(|(id, message)| {
                    let mut value = crate::application::client::message_to_sdk(message);
                    value.input_id = Some(id);
                    value
                })
                .collect(),
        },
        crate::application::loop_engine::chat::RuntimeStreamEvent::Done { context } => {
            ChatEvent::Done {
                context: turn_context_to_sdk(context),
            }
        }
        crate::application::loop_engine::chat::RuntimeStreamEvent::DoneWithDuration {
            context,
            duration,
        } => ChatEvent::DoneWithDurationMs {
            context: turn_context_to_sdk(context),
            duration_ms: duration.as_millis() as u64,
        },
        crate::application::loop_engine::chat::RuntimeStreamEvent::RunStarted {
            run_id,
            parent_run_id,
        } => ChatEvent::RunStarted {
            run_id,
            parent_run_id,
        },
        crate::application::loop_engine::chat::RuntimeStreamEvent::Cancelled {
            context,
            duration,
        } => ChatEvent::Cancelled {
            context: turn_context_to_sdk(context),
            duration_ms: duration.as_millis() as u64,
        },
        crate::application::loop_engine::chat::RuntimeStreamEvent::LiveTps(tps) => {
            ChatEvent::LiveTps(tps)
        }
        crate::application::loop_engine::chat::RuntimeStreamEvent::RunChanged(run_step) => {
            ChatEvent::CurrentRunChanged(run_step)
        }
        crate::application::loop_engine::chat::RuntimeStreamEvent::InteractionRequested {
            request,
        } => ChatEvent::InteractionRequested { request },
        crate::application::loop_engine::chat::RuntimeStreamEvent::AgentProgress {
            source_context,
            attachment_context,
            tool_id,
            event,
        } => ChatEvent::AgentProgress {
            source_context: turn_context_to_sdk(source_context),
            attachment_context: turn_context_to_sdk(attachment_context),
            tool_id,
            event: project_agent_progress_event(event),
        },
        crate::application::loop_engine::chat::RuntimeStreamEvent::ToolOutputDelta {
            context,
            tool_id,
            delta,
        } => ChatEvent::ToolOutputDelta {
            context: turn_context_to_sdk(context),
            tool_id,
            delta,
        },
        crate::application::loop_engine::chat::RuntimeStreamEvent::ChildRunActivity(event) => {
            ChatEvent::ChildRunActivity {
                event: child_run_activity_to_sdk(event),
            }
        }
        crate::application::loop_engine::chat::RuntimeStreamEvent::SkillsUpdated { snapshot } => {
            ChatEvent::SkillsUpdated {
                event: crate::application::client::skill_snapshot_to_sdk(snapshot),
            }
        }
        crate::application::loop_engine::chat::RuntimeStreamEvent::WorkingDirectoryChanged {
            path_base,
            workspace_root,
            workspace,
        } => ChatEvent::WorkingDirectoryChanged {
            path_base,
            workspace_root,
            workspace: crate::application::client::workspace_context_to_sdk(workspace),
        },
        crate::application::loop_engine::chat::RuntimeStreamEvent::ConfigReloaded {
            changed_keys,
            view,
        } => {
            let scopes = changed_keys
                .iter()
                .filter_map(|key| match key.as_str() {
                    "config:scope:immediate" => Some(sdk::ConfigApplicationScopeView::Immediate),
                    "config:scope:session_restart_required" => {
                        Some(sdk::ConfigApplicationScopeView::SessionRestartRequired)
                    }
                    "config:scope:run" => Some(sdk::ConfigApplicationScopeView::Run),
                    _ => None,
                })
                .collect();
            ChatEvent::ConfigReloaded {
                event: sdk::ConfigReloadedEvent {
                    changed_keys,
                    scopes,
                    view,
                },
            }
        }
        crate::application::loop_engine::chat::RuntimeStreamEvent::SessionReset => {
            ChatEvent::SessionReset
        }
        crate::application::loop_engine::chat::RuntimeStreamEvent::UserMessagesWithdrawn {
            texts,
        } => ChatEvent::UserMessagesWithdrawn { texts },
        crate::application::loop_engine::chat::RuntimeStreamEvent::ModelSwitched { result } => {
            ChatEvent::ModelSwitched { result }
        }
        crate::application::loop_engine::chat::RuntimeStreamEvent::ThinkingChanged { enabled } => {
            ChatEvent::ThinkingChanged { enabled }
        }
        crate::application::loop_engine::chat::RuntimeStreamEvent::ContextEstimated {
            estimate,
            message_count,
        } => ChatEvent::ContextEstimated {
            estimate,
            message_count,
        },
        crate::application::loop_engine::chat::RuntimeStreamEvent::CommandResultText {
            text,
            is_error,
        } => ChatEvent::CommandResultText { text, is_error },
        crate::application::loop_engine::chat::RuntimeStreamEvent::SessionResumed {
            steps,
            display_history,
            session_id,
            created_at,
            compacted,
        } => ChatEvent::SessionResumed {
            steps: steps
                .into_iter()
                .map(|step| sdk::ResumedSessionStep {
                    run_id: step.run_id,
                    step_id: step.step_id,
                    messages: step
                        .message_segments
                        .into_iter()
                        .flat_map(|segment| segment.iter().cloned().collect::<Vec<_>>())
                        .map(crate::application::client::message_to_sdk)
                        .collect(),
                    finalize_cause: step
                        .finalize_cause
                        .map(crate::application::client::map_finalize_cause_to_sdk),
                    duration_ms: step.duration_ms,
                })
                .collect(),
            display_history: display_history.map(map_display_history_index),
            session_id,
            created_at,
            compacted,
        },
        crate::application::loop_engine::chat::RuntimeStreamEvent::SessionResumeFailed {
            kind,
            id,
            message,
        } => ChatEvent::SessionResumeFailed { kind, id, message },
        crate::application::loop_engine::chat::RuntimeStreamEvent::ReflectionHistory {
            records,
        } => ChatEvent::ReflectionHistory { records },
        crate::application::loop_engine::chat::RuntimeStreamEvent::ModelList { models } => {
            ChatEvent::ModelList { models }
        }
        crate::application::loop_engine::chat::RuntimeStreamEvent::ReminderList { reminders } => {
            ChatEvent::ReminderList { reminders }
        }
        crate::application::loop_engine::chat::RuntimeStreamEvent::SessionList { sessions } => {
            ChatEvent::SessionList { sessions }
        }
        crate::application::loop_engine::chat::RuntimeStreamEvent::ProjectInfo { project } => {
            ChatEvent::ProjectInfo { project }
        }
        crate::application::loop_engine::chat::RuntimeStreamEvent::TaskStateChanged { state } => {
            ChatEvent::TaskStateChanged { state }
        }
        crate::application::loop_engine::chat::RuntimeStreamEvent::RuntimeStatusChanged {
            status,
        } => ChatEvent::RuntimeStatusChanged { status },
        crate::application::loop_engine::chat::RuntimeStreamEvent::CostUpdate { cost } => {
            ChatEvent::CostUpdate { cost }
        }
    }
}

fn child_run_activity_to_sdk(event: tools::ChildRunActivityEvent) -> ChildRunActivityEventView {
    ChildRunActivityEventView {
        identity: ChildRunIdentityView {
            agent_id: sdk::AgentId::from_legacy_or_new(&event.identity.agent_id),
            run_id: sdk::RunId::from_legacy_or_new(&event.identity.run_id),
            parent_run_id: sdk::RunId::from_legacy_or_new(&event.identity.parent_run_id),
            spawned_by_tool_call_id: sdk::ToolCallId::from_legacy_or_new(
                &event.identity.spawned_by_tool_call_id,
            ),
        },
        sequence: event.sequence,
        kind: match event.kind {
            tools::ChildRunActivityKind::Text { text } => ChildRunActivityKindView::Text { text },
            tools::ChildRunActivityKind::Thinking { text } => {
                ChildRunActivityKindView::Thinking { text }
            }
            tools::ChildRunActivityKind::ToolCall { id, name, input } => {
                ChildRunActivityKindView::ToolCall {
                    id: sdk::ToolCallId::from_legacy_or_new(&id),
                    name,
                    input,
                }
            }
            tools::ChildRunActivityKind::ToolOutput { tool_name, text } => {
                ChildRunActivityKindView::ToolOutput { tool_name, text }
            }
            tools::ChildRunActivityKind::ToolResult {
                tool_call_id,
                tool_name,
                output,
                content,
                is_error,
            } => ChildRunActivityKindView::ToolResult {
                tool_call_id: sdk::ToolCallId::from_legacy_or_new(&tool_call_id),
                tool_name,
                output,
                content,
                is_error,
            },
            tools::ChildRunActivityKind::Terminal { outcome } => {
                ChildRunActivityKindView::Terminal {
                    outcome: match outcome {
                        tools::ChildRunTerminalOutcome::Completed => {
                            ChildRunTerminalOutcomeView::Completed
                        }
                        tools::ChildRunTerminalOutcome::Failed { error } => {
                            ChildRunTerminalOutcomeView::Failed { error }
                        }
                        tools::ChildRunTerminalOutcome::Cancelled => {
                            ChildRunTerminalOutcomeView::Cancelled
                        }
                    },
                }
            }
        },
    }
}

pub(crate) fn project_agent_progress_event(
    event: tools::AgentProgressEvent,
) -> AgentProgressEventView {
    let kind = match event.kind {
        tools::AgentProgressKind::ToolCalls { calls } => AgentProgressKindView::ToolCalls {
            calls: calls
                .into_iter()
                .map(|call| AgentToolCallProgressView {
                    id: sdk::ToolCallId::from_legacy_or_new(&call.id),
                    name: call.name,
                    input: call.input,
                })
                .collect(),
        },
        tools::AgentProgressKind::ToolOutput { tool_name, text } => {
            AgentProgressKindView::ToolOutput { tool_name, text }
        }
        tools::AgentProgressKind::Message { text }
        | tools::AgentProgressKind::Thinking { text } => AgentProgressKindView::Message { text },
        tools::AgentProgressKind::ToolResult { tool_name, .. } => {
            AgentProgressKindView::ToolOutput {
                tool_name,
                text: String::new(),
            }
        }
        tools::AgentProgressKind::Terminal { outcome } => AgentProgressKindView::Message {
            text: format!("Sub-agent terminal: {outcome:?}"),
        },
        tools::AgentProgressKind::Started { role, model } => {
            AgentProgressKindView::Started { role, model }
        }
    };
    AgentProgressEventView {
        sequence: event.sequence,
        kind,
    }
}

#[cfg(test)]
mod run_status_mapping_tests {
    use super::map_lifecycle_event;
    use crate::domain::agent_run::{RunStatus, RunTransitionReason, RuntimeLifecycleEvent};
    use sdk::{ChatEvent, RunStatusView};

    #[test]
    fn transitioned_status_maps_every_runtime_variant() {
        let statuses = [
            (RunStatus::Created, RunStatusView::Created),
            (RunStatus::DrainingInput, RunStatusView::DrainingInput),
            (RunStatus::PreparingContext, RunStatusView::PreparingContext),
            (RunStatus::InvokingModel, RunStatusView::InvokingModel),
            (RunStatus::ApplyingResponse, RunStatusView::ApplyingResponse),
            (
                RunStatus::AwaitingToolApproval,
                RunStatusView::AwaitingToolApproval,
            ),
            (RunStatus::ExecutingTools, RunStatusView::ExecutingTools),
            (RunStatus::AwaitingUser, RunStatusView::AwaitingUser),
            (RunStatus::Compacting, RunStatusView::Compacting),
            (RunStatus::CancellingStep, RunStatusView::CancellingStep),
            (RunStatus::FinalizingStep, RunStatusView::FinalizingStep),
            (RunStatus::Terminating, RunStatusView::Terminating),
            (RunStatus::Completed, RunStatusView::Completed),
            (RunStatus::Failed, RunStatusView::Failed),
            (RunStatus::Terminated, RunStatusView::Terminated),
        ];

        for (runtime_status, expected_status) in statuses {
            let run_id = sdk::RunId::new_v7();
            let parent_run_id = sdk::RunId::new_v7();
            let event = RuntimeLifecycleEvent::Transitioned {
                run_id: run_id.clone(),
                parent_run_id: Some(parent_run_id.clone()),
                from: RunStatus::Created,
                to: runtime_status,
                reason: RunTransitionReason::DrainStarted,
                timing: crate::domain::agent_run::RunTimingSnapshot {
                    observation_revision: 7,
                    total_elapsed_ms: 12_345,
                    phase_elapsed_ms: 678,
                },
            };

            match map_lifecycle_event(event) {
                ChatEvent::RunTransitioned {
                    run_id: mapped_run_id,
                    parent_run_id: mapped_parent_run_id,
                    status,
                    timing,
                } => {
                    assert_eq!(mapped_run_id, run_id);
                    assert_eq!(mapped_parent_run_id, Some(parent_run_id));
                    assert_eq!(status, expected_status);
                    assert_eq!(timing.observation_revision, 7);
                    assert_eq!(timing.total_elapsed_ms, 12_345);
                    assert_eq!(timing.phase_elapsed_ms, 678);
                }
                other => panic!("expected RunTransitioned, got {other:?}"),
            }
        }
    }
}
