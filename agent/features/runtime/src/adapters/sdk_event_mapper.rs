//! Runtime-owned mappers to the SDK Published Language.

use crate::application::loop_engine::chat::RuntimeTurnContext;
use crate::application::loop_engine::chat::{
    RuntimeHookEvent, RuntimeHookEventStatus, RuntimeHookMessage, RuntimeHookMessageKind,
};
use crate::domain::agent_run::RunDomainEvent;
use sdk::{
    AgentProgressEventView, AgentProgressKindView, AgentToolCallProgressView, ChatEvent,
    ChatEventContext, HookEventStatus, HookEventView, HookExecutionResultView, HookMessageKindView,
    HookMessageView, ToolCallStatusView, ToolResultImage,
};

pub fn map_domain_event(event: RunDomainEvent) -> ChatEvent {
    match event {
        RunDomainEvent::Started {
            run_id,
            parent_run_id,
        } => ChatEvent::RunStarted {
            run_id,
            parent_run_id,
        },
        RunDomainEvent::StepStarted {
            run_id,
            parent_run_id,
            step_id,
        } => ChatEvent::RunStepStarted {
            run_id,
            parent_run_id,
            step_id,
        },
        RunDomainEvent::StepCompleted {
            run_id,
            parent_run_id,
            step_id,
        } => ChatEvent::RunStepCompleted {
            run_id,
            parent_run_id,
            step_id,
        },
        RunDomainEvent::StepCancellationRequested {
            run_id,
            parent_run_id,
            step_id,
        } => ChatEvent::RunStepCancellationRequested {
            run_id,
            parent_run_id,
            step_id,
        },
        RunDomainEvent::StepFinalizationStarted {
            run_id,
            parent_run_id,
            step_id,
        } => ChatEvent::RunStepFinalizationStarted {
            run_id,
            parent_run_id,
            step_id,
        },
        RunDomainEvent::StepCancelled {
            run_id,
            parent_run_id,
            step_id,
            confirmed,
        } => ChatEvent::RunStepCancelled {
            run_id,
            parent_run_id,
            step_id,
            confirmed,
        },
        RunDomainEvent::DrainingInput {
            run_id,
            parent_run_id,
        } => ChatEvent::RunDrainingInput {
            run_id,
            parent_run_id,
        },
        RunDomainEvent::TerminationRequested {
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
        RunDomainEvent::Terminated {
            run_id,
            parent_run_id,
            reason,
        } => ChatEvent::RunTerminated {
            run_id,
            parent_run_id,
            reason,
        },
        RunDomainEvent::Completed {
            run_id,
            parent_run_id,
            result,
            ..
        } => ChatEvent::RunCompleted {
            run_id,
            parent_run_id,
            result,
        },
        RunDomainEvent::Failed {
            run_id,
            parent_run_id,
            error,
        } => ChatEvent::RunFailed {
            run_id,
            parent_run_id,
            error,
        },
        RunDomainEvent::StuckDetected {
            run_id,
            parent_run_id,
            reason,
        } => ChatEvent::RunStuckDetected {
            run_id,
            parent_run_id,
            reason,
        },
        RunDomainEvent::CancellationRequested { run_id, .. } => ChatEvent::RunCancelling { run_id },
        RunDomainEvent::Cancelled { run_id, .. } => ChatEvent::RunCancelled { run_id },
        RunDomainEvent::Transitioned {
            run_id,
            parent_run_id,
            to,
            ..
        } => ChatEvent::RunTransitioned {
            run_id,
            parent_run_id,
            status: format!("{to:?}"),
        },
        RunDomainEvent::AwaitingUser {
            run_id,
            parent_run_id,
            ..
        } => ChatEvent::RunAwaitingUser {
            run_id,
            parent_run_id,
        },
        RunDomainEvent::Resumed {
            run_id,
            parent_run_id,
            ..
        } => ChatEvent::RunResumed {
            run_id,
            parent_run_id,
        },
    }
}

fn turn_context_to_sdk(context: RuntimeTurnContext) -> ChatEventContext {
    ChatEventContext::new(context.chat_id, context.turn_id)
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
                finalize_cause: step
                    .finalize_cause()
                    .map(crate::application::client::map_finalize_cause_to_sdk),
                duration_ms: step.duration_ms(),
            })
            .collect(),
    }
}

pub(crate) fn map_stream_event(
    event: crate::application::loop_engine::chat::RuntimeStreamEvent,
) -> ChatEvent {
    match event {
        crate::application::loop_engine::chat::RuntimeStreamEvent::Text { context, text } => {
            ChatEvent::Token {
                context: turn_context_to_sdk(context),
                text,
            }
        }
        crate::application::loop_engine::chat::RuntimeStreamEvent::Thinking { context, text } => {
            ChatEvent::Thinking {
                context: turn_context_to_sdk(context),
                text,
            }
        }
        crate::application::loop_engine::chat::RuntimeStreamEvent::BlockComplete {
            context,
            text,
        } => ChatEvent::BlockComplete {
            context: turn_context_to_sdk(context),
            text,
        },
        crate::application::loop_engine::chat::RuntimeStreamEvent::ToolCallStart {
            context,
            id,
            provider_id,
            name,
            index,
        } => ChatEvent::ToolCallStart {
            context: turn_context_to_sdk(context),
            id,
            provider_id,
            name,
            index,
        },
        crate::application::loop_engine::chat::RuntimeStreamEvent::ToolCallUpdate {
            context,
            id,
            provider_id,
            name,
            index,
            arguments_delta,
            arguments,
            status,
        } => ChatEvent::ToolCallUpdate {
            context: turn_context_to_sdk(context),
            id,
            provider_id,
            name,
            index,
            arguments_delta,
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
        crate::application::loop_engine::chat::RuntimeStreamEvent::ModelStreamWaiting {
            context,
            elapsed_secs,
            phase,
        } => ChatEvent::ModelStreamWaiting {
            context: turn_context_to_sdk(context),
            elapsed_secs,
            phase,
        },
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
        crate::application::loop_engine::chat::RuntimeStreamEvent::MicrocompactDone {
            messages,
            cleared_count,
        } => ChatEvent::MicrocompactDone {
            messages: messages
                .into_iter()
                .map(crate::application::client::message_to_sdk)
                .collect(),
            cleared_count,
        },
        crate::application::loop_engine::chat::RuntimeStreamEvent::StopHookBlocked { messages } => {
            ChatEvent::StopHookBlocked {
                messages: messages
                    .into_iter()
                    .map(crate::application::client::message_to_sdk)
                    .collect(),
            }
        }
        crate::application::loop_engine::chat::RuntimeStreamEvent::PostToolExecutionSync {
            messages,
        } => ChatEvent::PostToolExecutionSync {
            messages: messages
                .into_iter()
                .map(crate::application::client::message_to_sdk)
                .collect(),
        },
        crate::application::loop_engine::chat::RuntimeStreamEvent::ApiError { messages, error } => {
            ChatEvent::ApiError {
                messages: messages
                    .into_iter()
                    .map(crate::application::client::message_to_sdk)
                    .collect(),
                error,
            }
        }
        crate::application::loop_engine::chat::RuntimeStreamEvent::CompactRollback { messages } => {
            ChatEvent::CompactRollback {
                messages: messages
                    .into_iter()
                    .map(crate::application::client::message_to_sdk)
                    .collect(),
            }
        }
        crate::application::loop_engine::chat::RuntimeStreamEvent::CompactFinished { messages } => {
            ChatEvent::CompactFinished {
                messages: messages
                    .into_iter()
                    .map(crate::application::client::message_to_sdk)
                    .collect(),
            }
        }
        crate::application::loop_engine::chat::RuntimeStreamEvent::UserMessagesAdopted {
            items,
            queued,
        } => ChatEvent::UserMessagesAdopted {
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
        },
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
        crate::application::loop_engine::chat::RuntimeStreamEvent::RunCancelling { run_id } => {
            ChatEvent::RunCancelling { run_id }
        }
        crate::application::loop_engine::chat::RuntimeStreamEvent::RunCancelled { run_id } => {
            ChatEvent::RunCancelled { run_id }
        }
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
        crate::application::loop_engine::chat::RuntimeStreamEvent::TurnChanged(turn) => {
            ChatEvent::CurrentTurnChanged(turn)
        }
        crate::application::loop_engine::chat::RuntimeStreamEvent::HookEvent(event) => {
            ChatEvent::HookEvent(project_hook_event(event))
        }
        crate::application::loop_engine::chat::RuntimeStreamEvent::HookMessage(message) => {
            ChatEvent::HookMessage(project_hook_message(message))
        }
        crate::application::loop_engine::chat::RuntimeStreamEvent::AskUserBatch {
            items,
            reply_tx,
        } => ChatEvent::AskUserBatch { items, reply_tx },
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
        crate::application::loop_engine::chat::RuntimeStreamEvent::CompactProgress {
            stage,
            current,
            total,
        } => ChatEvent::CompactProgress {
            stage: stage.as_str().to_string(),
            current: current.map(|n| n as u32),
            total: total.map(|n| n as u32),
        },
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
        crate::application::loop_engine::chat::RuntimeStreamEvent::TasksSnapshot { tasks } => {
            ChatEvent::TasksSnapshot { tasks }
        }
        crate::application::loop_engine::chat::RuntimeStreamEvent::CostUpdate { cost } => {
            ChatEvent::CostUpdate { cost }
        }
    }
}

pub(crate) fn project_hook_event(event: RuntimeHookEvent) -> HookEventView {
    HookEventView {
        hook_name: event.hook_name,
        status: hook_event_status_to_sdk(event.status),
        matcher: event.matcher,
        command: event.command,
        result: event.result.map(|result| HookExecutionResultView {
            exit_code: result.exit_code,
            stdout: result.stdout,
            stderr: result.stderr,
            decision: result.decision,
            reason: result.reason,
            additional_context: result.additional_context,
        }),
    }
}

pub(crate) fn project_hook_message(message: RuntimeHookMessage) -> HookMessageView {
    HookMessageView {
        point: format!("{:?}", message.point),
        source: message.source,
        execution_ordinal: message.execution_ordinal,
        attempt: message.attempt,
        kind: project_hook_message_kind(message.kind),
        text: message.text,
    }
}

fn project_hook_message_kind(kind: RuntimeHookMessageKind) -> HookMessageKindView {
    match kind {
        RuntimeHookMessageKind::AdditionalContext => HookMessageKindView::AdditionalContext,
        RuntimeHookMessageKind::SystemMessage => HookMessageKindView::SystemMessage,
    }
}

fn hook_event_status_to_sdk(status: RuntimeHookEventStatus) -> HookEventStatus {
    match status {
        RuntimeHookEventStatus::Running => HookEventStatus::Running,
        RuntimeHookEventStatus::Succeeded => HookEventStatus::Succeeded,
        RuntimeHookEventStatus::Blocked => HookEventStatus::Blocked,
        RuntimeHookEventStatus::Failed => HookEventStatus::Failed,
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
        tools::AgentProgressKind::Message { text } => AgentProgressKindView::Message { text },
        tools::AgentProgressKind::Started { role, model } => {
            AgentProgressKindView::Started { role, model }
        }
    };
    AgentProgressEventView {
        sequence: event.sequence,
        kind,
    }
}
