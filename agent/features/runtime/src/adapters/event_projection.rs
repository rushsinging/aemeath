//! Runtime-owned projections to the SDK Published Language.

use crate::application::main_loop::looping::RuntimeTurnContext;
use crate::application::main_loop::{
    RuntimeHookEvent, RuntimeHookEventStatus, RuntimeHookMessage, RuntimeHookMessageKind,
};
use crate::domain::agent_run::RunDomainEvent;
use sdk::{
    AgentProgressEventView, AgentProgressKindView, AgentToolCallProgressView, ChatEvent,
    ChatEventContext, HookEventStatus, HookEventView, HookExecutionResultView, HookMessageKindView,
    HookMessageView, ToolCallStatusView, ToolResultImage,
};

pub fn project_domain_event(event: RunDomainEvent) -> ChatEvent {
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
    status: crate::application::main_loop::RuntimeToolCallStatus,
) -> ToolCallStatusView {
    match status {
        crate::application::main_loop::RuntimeToolCallStatus::PendingArgs => {
            ToolCallStatusView::PendingArgs
        }
        crate::application::main_loop::RuntimeToolCallStatus::Ready => ToolCallStatusView::Ready,
        crate::application::main_loop::RuntimeToolCallStatus::Running => {
            ToolCallStatusView::Running
        }
    }
}

pub(crate) fn project_stream_event(
    event: crate::application::main_loop::RuntimeStreamEvent,
) -> ChatEvent {
    match event {
        crate::application::main_loop::RuntimeStreamEvent::Text { context, text } => {
            ChatEvent::Token {
                context: turn_context_to_sdk(context),
                text,
            }
        }
        crate::application::main_loop::RuntimeStreamEvent::Thinking { context, text } => {
            ChatEvent::Thinking {
                context: turn_context_to_sdk(context),
                text,
            }
        }
        crate::application::main_loop::RuntimeStreamEvent::BlockComplete { context, text } => {
            ChatEvent::BlockComplete {
                context: turn_context_to_sdk(context),
                text,
            }
        }
        crate::application::main_loop::RuntimeStreamEvent::ToolCallStart {
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
        crate::application::main_loop::RuntimeStreamEvent::ToolCallUpdate {
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
        crate::application::main_loop::RuntimeStreamEvent::ToolResult {
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
        crate::application::main_loop::RuntimeStreamEvent::SystemMessage(msg) => {
            ChatEvent::SystemMessage(msg)
        }
        crate::application::main_loop::RuntimeStreamEvent::ModelStreamWaiting {
            context,
            elapsed_secs,
            phase,
        } => ChatEvent::ModelStreamWaiting {
            context: turn_context_to_sdk(context),
            elapsed_secs,
            phase,
        },
        crate::application::main_loop::RuntimeStreamEvent::ModelInvocationRetrying {
            context,
            attempt,
            delay,
        } => ChatEvent::ModelInvocationRetrying {
            context: turn_context_to_sdk(context),
            attempt,
            delay,
        },
        crate::application::main_loop::RuntimeStreamEvent::Usage {
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
        crate::application::main_loop::RuntimeStreamEvent::TurnStarted { messages } => {
            ChatEvent::TurnStarted {
                messages: messages
                    .into_iter()
                    .map(crate::application::client::message_to_sdk)
                    .collect(),
            }
        }
        crate::application::main_loop::RuntimeStreamEvent::MicrocompactDone {
            messages,
            cleared_count,
        } => ChatEvent::MicrocompactDone {
            messages: messages
                .into_iter()
                .map(crate::application::client::message_to_sdk)
                .collect(),
            cleared_count,
        },
        crate::application::main_loop::RuntimeStreamEvent::StopHookBlocked { messages } => {
            ChatEvent::StopHookBlocked {
                messages: messages
                    .into_iter()
                    .map(crate::application::client::message_to_sdk)
                    .collect(),
            }
        }
        crate::application::main_loop::RuntimeStreamEvent::PostToolExecutionSync { messages } => {
            ChatEvent::PostToolExecutionSync {
                messages: messages
                    .into_iter()
                    .map(crate::application::client::message_to_sdk)
                    .collect(),
            }
        }
        crate::application::main_loop::RuntimeStreamEvent::ApiError { messages, error } => {
            ChatEvent::ApiError {
                messages: messages
                    .into_iter()
                    .map(crate::application::client::message_to_sdk)
                    .collect(),
                error,
            }
        }
        crate::application::main_loop::RuntimeStreamEvent::CompactRollback { messages } => {
            ChatEvent::CompactRollback {
                messages: messages
                    .into_iter()
                    .map(crate::application::client::message_to_sdk)
                    .collect(),
            }
        }
        crate::application::main_loop::RuntimeStreamEvent::CompactFinished { messages } => {
            ChatEvent::CompactFinished {
                messages: messages
                    .into_iter()
                    .map(crate::application::client::message_to_sdk)
                    .collect(),
            }
        }
        crate::application::main_loop::RuntimeStreamEvent::UserMessagesAdopted {
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
        crate::application::main_loop::RuntimeStreamEvent::UserMessagesQueued { queued } => {
            ChatEvent::UserMessagesQueued {
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
        crate::application::main_loop::RuntimeStreamEvent::Done { context } => ChatEvent::Done {
            context: turn_context_to_sdk(context),
        },
        crate::application::main_loop::RuntimeStreamEvent::DoneWithDuration {
            context,
            duration,
        } => ChatEvent::DoneWithDurationMs {
            context: turn_context_to_sdk(context),
            duration_ms: duration.as_millis() as u64,
        },
        crate::application::main_loop::RuntimeStreamEvent::RunStarted {
            run_id,
            parent_run_id,
        } => ChatEvent::RunStarted {
            run_id,
            parent_run_id,
        },
        crate::application::main_loop::RuntimeStreamEvent::RunCancelling { run_id } => {
            ChatEvent::RunCancelling { run_id }
        }
        crate::application::main_loop::RuntimeStreamEvent::RunCancelled { run_id } => {
            ChatEvent::RunCancelled { run_id }
        }
        crate::application::main_loop::RuntimeStreamEvent::Cancelled { context } => {
            ChatEvent::Cancelled {
                context: turn_context_to_sdk(context),
            }
        }
        crate::application::main_loop::RuntimeStreamEvent::LiveTps(tps) => ChatEvent::LiveTps(tps),
        crate::application::main_loop::RuntimeStreamEvent::TurnChanged(turn) => {
            ChatEvent::CurrentTurnChanged(turn)
        }
        crate::application::main_loop::RuntimeStreamEvent::HookEvent(event) => {
            ChatEvent::HookEvent(project_hook_event(event))
        }
        crate::application::main_loop::RuntimeStreamEvent::HookMessage(message) => {
            ChatEvent::HookMessage(project_hook_message(message))
        }
        crate::application::main_loop::RuntimeStreamEvent::AskUserBatch { items, reply_tx } => {
            ChatEvent::AskUserBatch { items, reply_tx }
        }
        crate::application::main_loop::RuntimeStreamEvent::InteractionRequested { request } => {
            ChatEvent::InteractionRequested { request }
        }
        crate::application::main_loop::RuntimeStreamEvent::AgentProgress {
            context,
            tool_id,
            event,
        } => ChatEvent::AgentProgress {
            context: turn_context_to_sdk(context),
            tool_id,
            event: project_agent_progress_event(event),
        },
        crate::application::main_loop::RuntimeStreamEvent::WorkingDirectoryChanged {
            path_base,
            workspace_root,
            workspace,
        } => ChatEvent::WorkingDirectoryChanged {
            path_base,
            workspace_root,
            workspace: crate::application::client::workspace_context_to_sdk(workspace),
        },
        crate::application::main_loop::RuntimeStreamEvent::ConfigReloaded { changed_keys } => {
            let scopes = changed_keys
                .iter()
                .filter_map(|key| match key.as_str() {
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
                },
            }
        }
        crate::application::main_loop::RuntimeStreamEvent::GraphPhaseChanged {
            node,
            effort,
            prev,
        } => ChatEvent::GraphPhaseChanged {
            node: format!("{node}"),
            effort: format!("{effort:?}").to_lowercase(),
            prev: format!("{prev}"),
        },
        crate::application::main_loop::RuntimeStreamEvent::SessionReset => ChatEvent::SessionReset,
        crate::application::main_loop::RuntimeStreamEvent::UserMessagesWithdrawn { texts } => {
            ChatEvent::UserMessagesWithdrawn { texts }
        }
        crate::application::main_loop::RuntimeStreamEvent::CompactProgress {
            stage,
            current,
            total,
        } => ChatEvent::CompactProgress {
            stage: stage.as_str().to_string(),
            current: current.map(|n| n as u32),
            total: total.map(|n| n as u32),
        },
        crate::application::main_loop::RuntimeStreamEvent::ModelSwitched { result } => {
            ChatEvent::ModelSwitched { result }
        }
        crate::application::main_loop::RuntimeStreamEvent::ThinkingChanged { enabled } => {
            ChatEvent::ThinkingChanged { enabled }
        }
        crate::application::main_loop::RuntimeStreamEvent::ContextEstimated {
            estimate,
            message_count,
        } => ChatEvent::ContextEstimated {
            estimate,
            message_count,
        },
        crate::application::main_loop::RuntimeStreamEvent::CommandResultText { text, is_error } => {
            ChatEvent::CommandResultText { text, is_error }
        }
        crate::application::main_loop::RuntimeStreamEvent::SessionResumed {
            steps,
            session_id,
            created_at,
        } => ChatEvent::SessionResumed {
            steps: steps
                .into_iter()
                .map(|step| sdk::ResumedSessionStep {
                    run_id: step.run_id,
                    step_id: step.step_id,
                    messages: step
                        .messages
                        .into_iter()
                        .map(crate::application::client::message_to_sdk)
                        .collect(),
                })
                .collect(),
            session_id,
            created_at,
        },
        crate::application::main_loop::RuntimeStreamEvent::SessionResumeFailed {
            kind,
            id,
            message,
        } => ChatEvent::SessionResumeFailed { kind, id, message },
        crate::application::main_loop::RuntimeStreamEvent::ReflectionHistory { records } => {
            ChatEvent::ReflectionHistory { records }
        }
        crate::application::main_loop::RuntimeStreamEvent::ModelList { models } => {
            ChatEvent::ModelList { models }
        }
        crate::application::main_loop::RuntimeStreamEvent::ReminderList { reminders } => {
            ChatEvent::ReminderList { reminders }
        }
        crate::application::main_loop::RuntimeStreamEvent::SessionList { sessions } => {
            ChatEvent::SessionList { sessions }
        }
        crate::application::main_loop::RuntimeStreamEvent::ProjectInfo { project } => {
            ChatEvent::ProjectInfo { project }
        }
        crate::application::main_loop::RuntimeStreamEvent::TasksSnapshot { tasks } => {
            ChatEvent::TasksSnapshot { tasks }
        }
        crate::application::main_loop::RuntimeStreamEvent::CostUpdate { cost } => {
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
