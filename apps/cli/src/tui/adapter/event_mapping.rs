use super::runtime_view::{
    TuiChatMessage, TuiContentBlock, TuiMessageSource, TuiStopHookFeedback, TuiToolResultImage,
};
use super::tui_runtime_event::*;
use crate::tui::model::conversation::interaction::{UiInteractionRequestId, UiRunId, UiRunStepId};

#[allow(clippy::large_enum_variant)]
pub(crate) enum SdkEventMapping {
    Runtime(TuiRuntimeEvent),
    /// Events that have been fully retired and carry no TUI-relevant payload.
    Nop,
}

pub(crate) fn sdk_event_to_tui_event(event: sdk::ChatEvent) -> SdkEventMapping {
    use sdk::ChatEvent;

    let runtime = match event {
        ChatEvent::Token { context, text } => TuiRuntimeEvent::Text {
            context: turn_context(context),
            text,
        },
        ChatEvent::Thinking { context, text } => TuiRuntimeEvent::Thinking {
            context: turn_context(context),
            text,
        },
        ChatEvent::BlockComplete { context, text } => TuiRuntimeEvent::BlockComplete {
            context: turn_context(context),
            text,
        },
        ChatEvent::ToolCallStart {
            context,
            id,
            provider_id,
            name,
            index,
        } => TuiRuntimeEvent::ToolCallStart {
            context: turn_context(context),
            id: id.as_str().to_string(),
            provider_id,
            name,
            index,
        },
        ChatEvent::ToolCallUpdate {
            context,
            id,
            provider_id,
            name,
            index,
            arguments_delta,
            arguments,
            status,
        } => TuiRuntimeEvent::ToolCallUpdate {
            context: turn_context(context),
            id: id.as_str().to_string(),
            provider_id,
            name,
            index,
            arguments_delta,
            arguments,
            status: tool_status(status),
        },
        ChatEvent::ToolResult {
            context,
            id,
            provider_id,
            tool_name,
            output,
            content,
            is_error,
            images,
        } => TuiRuntimeEvent::ToolResult {
            context: turn_context(context),
            id: id.as_str().to_string(),
            provider_id,
            tool_name,
            output,
            content,
            is_error,
            images: images.into_iter().map(tool_image).collect(),
        },
        ChatEvent::SystemMessage(message) => TuiRuntimeEvent::SystemMessage(message),
        ChatEvent::ModelStreamWaiting {
            context,
            elapsed_secs,
            phase,
        } => TuiRuntimeEvent::ModelStreamWaiting {
            context: turn_context(context),
            elapsed_secs,
            phase,
        },
        ChatEvent::ModelInvocationRetrying {
            context,
            attempt,
            delay,
        } => TuiRuntimeEvent::ModelInvocationRetrying {
            context: turn_context(context),
            attempt,
            delay_ms: delay.as_millis(),
        },
        ChatEvent::Usage {
            input,
            output,
            last_input,
            elapsed_secs,
        } => TuiRuntimeEvent::Usage {
            input,
            output,
            last_input,
            elapsed_secs,
        },
        ChatEvent::TurnStarted { messages } => TuiRuntimeEvent::TurnStarted {
            messages: messages.into_iter().map(chat_message).collect(),
        },
        ChatEvent::MicrocompactDone {
            messages,
            cleared_count,
        } => TuiRuntimeEvent::MicrocompactDone {
            messages: messages.into_iter().map(chat_message).collect(),
            cleared_count,
        },
        ChatEvent::StopHookBlocked { messages } => TuiRuntimeEvent::StopHookBlocked {
            messages: messages.into_iter().map(chat_message).collect(),
        },
        ChatEvent::PostToolExecutionSync { messages } => TuiRuntimeEvent::PostToolExecutionSync {
            messages: messages.into_iter().map(chat_message).collect(),
        },
        ChatEvent::ApiError { messages, error } => TuiRuntimeEvent::ApiError {
            messages: messages.into_iter().map(chat_message).collect(),
            error,
        },
        ChatEvent::CompactRollback { messages } => TuiRuntimeEvent::CompactRollback {
            messages: messages.into_iter().map(chat_message).collect(),
        },
        ChatEvent::CompactFinished { messages } => TuiRuntimeEvent::CompactFinished {
            messages: messages.into_iter().map(chat_message).collect(),
        },
        ChatEvent::UserMessagesAdopted { items, queued } => TuiRuntimeEvent::UserMessagesAdopted {
            items: items.into_iter().map(chat_message).collect(),
            queued: queued.into_iter().map(chat_message).collect(),
        },
        ChatEvent::UserMessagesQueued { queued } => TuiRuntimeEvent::UserMessagesQueued {
            queued: queued.into_iter().map(chat_message).collect(),
        },
        ChatEvent::Done { context } => TuiRuntimeEvent::Done {
            context: turn_context(context),
            duration_ms: None,
        },
        ChatEvent::DoneWithDurationMs {
            context,
            duration_ms,
        } => TuiRuntimeEvent::Done {
            context: turn_context(context),
            duration_ms: Some(duration_ms),
        },
        ChatEvent::RunStarted {
            run_id,
            parent_run_id,
        } => run_event(run_id, parent_run_id, TuiRunEvent::Started),
        ChatEvent::RunAwaitingUser {
            run_id,
            parent_run_id,
        } => run_event(run_id, parent_run_id, TuiRunEvent::AwaitingUser),
        ChatEvent::RunResumed {
            run_id,
            parent_run_id,
        } => run_event(run_id, parent_run_id, TuiRunEvent::Resumed),
        ChatEvent::RunCancelling { run_id } => run_event(run_id, None, TuiRunEvent::Cancelling),
        ChatEvent::RunCancelled { run_id } => run_event(run_id, None, TuiRunEvent::Cancelled),
        ChatEvent::RunDrainingInput {
            run_id,
            parent_run_id,
        } => run_event(run_id, parent_run_id, TuiRunEvent::DrainingInput),
        ChatEvent::RunCompleted {
            run_id,
            parent_run_id,
            result,
        } => run_event(run_id, parent_run_id, TuiRunEvent::Completed { result }),
        ChatEvent::RunFailed {
            run_id,
            parent_run_id,
            error,
        } => run_event(run_id, parent_run_id, TuiRunEvent::Failed { error }),
        ChatEvent::RunStuckDetected {
            run_id,
            parent_run_id,
            reason,
        } => run_event(run_id, parent_run_id, TuiRunEvent::Stuck { reason }),
        ChatEvent::RunTransitioned {
            run_id,
            parent_run_id,
            status,
        } => run_event(run_id, parent_run_id, TuiRunEvent::Transitioned { status }),
        ChatEvent::RunTerminationRequested {
            run_id,
            parent_run_id,
            reason,
            deadline,
        } => run_event(
            run_id,
            parent_run_id,
            TuiRunEvent::TerminationRequested {
                reason: run_termination_reason(reason),
                deadline_unix_millis: deadline.unix_millis(),
            },
        ),
        ChatEvent::RunTerminated {
            run_id,
            parent_run_id,
            reason,
        } => run_event(
            run_id,
            parent_run_id,
            TuiRunEvent::Terminated {
                reason: run_termination_reason(reason),
            },
        ),
        ChatEvent::RunStepStarted {
            run_id,
            parent_run_id,
            step_id,
        } => run_step_event(run_id, parent_run_id, step_id, TuiRunStepEvent::Started),
        ChatEvent::RunStepCompleted {
            run_id,
            parent_run_id,
            step_id,
        } => run_step_event(run_id, parent_run_id, step_id, TuiRunStepEvent::Completed),
        ChatEvent::RunStepCancellationRequested {
            run_id,
            parent_run_id,
            step_id,
        } => run_step_event(
            run_id,
            parent_run_id,
            step_id,
            TuiRunStepEvent::CancellationRequested,
        ),
        ChatEvent::RunStepFinalizationStarted {
            run_id,
            parent_run_id,
            step_id,
        } => run_step_event(
            run_id,
            parent_run_id,
            step_id,
            TuiRunStepEvent::FinalizationStarted,
        ),
        ChatEvent::RunStepCancelled {
            run_id,
            parent_run_id,
            step_id,
            confirmed,
        } => run_step_event(
            run_id,
            parent_run_id,
            step_id,
            TuiRunStepEvent::Cancelled { confirmed },
        ),
        ChatEvent::InteractionRequested { request } => {
            TuiRuntimeEvent::InteractionRequested(interaction_request(request))
        }
        ChatEvent::Cancelled { context } => TuiRuntimeEvent::Cancelled {
            context: turn_context(context),
        },
        ChatEvent::LiveTps(tps) => TuiRuntimeEvent::LiveTps(tps),
        ChatEvent::TurnChanged(turn) | ChatEvent::CurrentTurnChanged(turn) => {
            TuiRuntimeEvent::TurnChanged(turn)
        }
        ChatEvent::HookEvent(event) => TuiRuntimeEvent::HookEvent(hook_event(event)),
        ChatEvent::HookMessage(message) => TuiRuntimeEvent::HookMessage(hook_message(message)),
        // #944 5B: AskUserBatch legacy bridge removed.
        ChatEvent::AskUserBatch { .. } => return SdkEventMapping::Nop,
        ChatEvent::AgentProgress {
            context,
            tool_id,
            event,
        } => TuiRuntimeEvent::AgentProgress {
            context: turn_context(context),
            tool_id: tool_id.as_str().to_string(),
            event: agent_progress(event),
        },
        ChatEvent::WorkingDirectoryChanged {
            path_base,
            workspace_root,
            workspace,
        } => TuiRuntimeEvent::WorkspaceSnapshot(TuiWorkspaceSnapshot {
            path_base,
            workspace_root,
            context_stack: workspace
                .context_stack
                .into_iter()
                .map(|entry| {
                    (
                        entry.path_base.to_string_lossy().into_owned(),
                        entry.workspace_root.to_string_lossy().into_owned(),
                    )
                })
                .collect(),
        }),
        ChatEvent::ConfigChanged { event } => TuiRuntimeEvent::ConfigChanged {
            cause: config_cause(event.cause),
            changed_fields: event.changed_fields.into_iter().map(config_field).collect(),
            view: config_view(event.view),
        },
        ChatEvent::ConfigReloaded { event } => TuiRuntimeEvent::ConfigReloaded {
            changed_keys: event.changed_keys,
        },
        ChatEvent::SessionReset => TuiRuntimeEvent::SessionReset,
        ChatEvent::UserMessagesWithdrawn { texts } => {
            TuiRuntimeEvent::UserMessagesWithdrawn { texts }
        }
        ChatEvent::GraphPhaseChanged { node, effort, prev } => TuiRuntimeEvent::GraphPhaseChanged {
            node,
            effort,
            previous: prev,
        },
        ChatEvent::Result(result) => TuiRuntimeEvent::CommandResultText {
            text: result.text,
            is_error: false,
        },
        ChatEvent::CompactProgress {
            stage,
            current,
            total,
        } => TuiRuntimeEvent::CompactProgress {
            stage,
            current,
            total,
        },
        ChatEvent::ModelSwitched { result } => TuiRuntimeEvent::ModelSwitched {
            display_name: result.display_name,
            context_window: result.context_window,
            reasoning_active: result.reasoning_active,
        },
        ChatEvent::ThinkingChanged { enabled } => TuiRuntimeEvent::ThinkingChanged { enabled },
        ChatEvent::ContextEstimated {
            estimate,
            message_count,
        } => TuiRuntimeEvent::ContextEstimated {
            estimated_tokens: estimate.estimated_tokens,
            system_tokens: estimate.system_tokens,
            context_size: estimate.context_size,
            usage_percentage: estimate.usage_percentage,
            message_count,
        },
        ChatEvent::CommandResultText { text, is_error } => {
            TuiRuntimeEvent::CommandResultText { text, is_error }
        }
        ChatEvent::SessionResumed {
            steps,
            session_id,
            created_at,
        } => TuiRuntimeEvent::SessionResumed {
            steps: steps
                .into_iter()
                .map(|step| super::runtime_view::TuiResumedSessionStep {
                    run_id: step.run_id,
                    step_id: step.step_id,
                    messages: step.messages.into_iter().map(chat_message).collect(),
                })
                .collect(),
            session_id,
            created_at,
        },
        ChatEvent::SessionResumeFailed { kind, id, message } => {
            TuiRuntimeEvent::SessionResumeFailed {
                kind: session_failure(kind),
                id,
                message,
            }
        }
        ChatEvent::ReflectionHistory { records } => TuiRuntimeEvent::ReflectionHistory {
            records: records.into_iter().map(reflection_record).collect(),
        },
        ChatEvent::ModelList { models } => TuiRuntimeEvent::ModelList {
            models: models
                .into_iter()
                .map(|model| TuiModelSummary {
                    provider: model.provider,
                    id: model.id,
                    name: model.name,
                    context_window: model.context_window,
                    max_tokens: model.max_tokens,
                })
                .collect(),
        },
        ChatEvent::ReminderList { reminders } => TuiRuntimeEvent::ReminderList {
            reminders: reminders
                .into_iter()
                .map(|reminder| TuiReminder {
                    id: reminder.id,
                    content: reminder.content,
                    done: reminder.done,
                    created_at: reminder.created_at,
                })
                .collect(),
        },
        ChatEvent::SessionList { sessions } => TuiRuntimeEvent::SessionList {
            sessions: sessions
                .into_iter()
                .map(|session| TuiSessionSummary {
                    id: session.id,
                    title: session.title,
                    project: session.project,
                    model: session.model,
                    created_at: session.created_at,
                    updated_at: session.updated_at,
                    message_count: session.message_count,
                    preview: session.preview,
                    summary: session.summary,
                })
                .collect(),
        },
        ChatEvent::ProjectInfo { project } => TuiRuntimeEvent::ProjectInfo {
            project: TuiProjectInfo {
                cwd: project.cwd,
                path_base: project.path_base,
                workspace_root: project.workspace_root,
                git_branch: project.git_branch,
            },
        },
        ChatEvent::TasksSnapshot { tasks } => TuiRuntimeEvent::TasksSnapshot { lines: tasks.lines },
        ChatEvent::CostUpdate { cost } => TuiRuntimeEvent::CostUpdate {
            input_tokens: cost.input_tokens,
            output_tokens: cost.output_tokens,
            cost_usd: cost.cost_usd,
        },
    };
    SdkEventMapping::Runtime(runtime)
}

fn turn_context(value: sdk::ChatEventContext) -> TuiTurnContext {
    TuiTurnContext {
        chat_id: value.chat_id.as_str().to_string(),
        turn_id: value.turn_id.as_str().to_string(),
    }
}
fn tool_status(value: sdk::ToolCallStatusView) -> TuiToolCallStatus {
    match value {
        sdk::ToolCallStatusView::PendingArgs => TuiToolCallStatus::PendingArgs,
        sdk::ToolCallStatusView::Ready => TuiToolCallStatus::Ready,
        sdk::ToolCallStatusView::Running => TuiToolCallStatus::Running,
    }
}
fn tool_image(value: sdk::ToolResultImage) -> TuiToolResultImage {
    TuiToolResultImage {
        base64: value.base64,
        media_type: value.media_type,
    }
}
fn run_id(value: sdk::RunId) -> UiRunId {
    UiRunId::from(value.as_str())
}
fn parent_run_id(value: Option<sdk::RunId>) -> Option<UiRunId> {
    value.as_ref().map(|id| UiRunId::from(id.as_str()))
}
fn run_event(
    run_id_value: sdk::RunId,
    parent: Option<sdk::RunId>,
    event: TuiRunEvent,
) -> TuiRuntimeEvent {
    TuiRuntimeEvent::Run {
        run_id: run_id(run_id_value),
        parent_run_id: parent_run_id(parent),
        event,
    }
}
fn run_step_event(
    run_id_value: sdk::RunId,
    parent: Option<sdk::RunId>,
    step_id: sdk::RunStepId,
    event: TuiRunStepEvent,
) -> TuiRuntimeEvent {
    TuiRuntimeEvent::RunStep {
        run_id: run_id(run_id_value),
        parent_run_id: parent_run_id(parent),
        step_id: UiRunStepId::from(step_id.as_str()),
        event,
    }
}

fn interaction_request(value: sdk::InteractionRequest) -> TuiInteractionRequest {
    TuiInteractionRequest {
        request_id: UiInteractionRequestId::from(value.id.as_str()),
        run_id: UiRunId::from(value.run_id.as_str()),
        body: match value.body {
            sdk::InteractionRequestBody::UserQuestions(questions) => {
                TuiInteractionBody::UserQuestions(
                    questions
                        .into_iter()
                        .map(|question| TuiUserQuestion {
                            prompt: question.prompt,
                            options: question.options,
                            allow_multi: question.allow_multi,
                        })
                        .collect(),
                )
            }
            sdk::InteractionRequestBody::ToolApproval(prompt) => {
                TuiInteractionBody::ToolApproval(TuiToolApprovalPrompt {
                    tool_name: prompt.tool_name,
                    args_summary: prompt.args_summary,
                    risk_level: match prompt.risk_level {
                        sdk::RiskLevel::Low => TuiRiskLevel::Low,
                        sdk::RiskLevel::Medium => TuiRiskLevel::Medium,
                        sdk::RiskLevel::High => TuiRiskLevel::High,
                    },
                })
            }
            sdk::InteractionRequestBody::PlanApproval(prompt) => {
                TuiInteractionBody::PlanApproval(TuiPlanApprovalPrompt {
                    plan_title: prompt.plan_title,
                    steps: prompt.steps,
                })
            }
            sdk::InteractionRequestBody::HardPause(diagnostic) => {
                TuiInteractionBody::HardPause(TuiStuckDiagnostic {
                    reason: diagnostic.reason,
                    recent_actions: diagnostic.recent_actions,
                })
            }
        },
    }
}

pub(crate) fn chat_message(value: sdk::ChatMessage) -> TuiChatMessage {
    let (source, stop_hook) = match value.metadata {
        Some(metadata) => (
            match metadata.source {
                sdk::ChatMessageSource::User => TuiMessageSource::User,
                sdk::ChatMessageSource::SystemGenerated => TuiMessageSource::SystemGenerated,
                sdk::ChatMessageSource::StopHook => TuiMessageSource::StopHook,
            },
            metadata.stop_hook.map(|hook| TuiStopHookFeedback {
                summary: hook.summary,
                command: hook.command,
                exit_code: hook.exit_code,
                reason: hook.reason,
                stdout_preview: hook.stdout_preview,
                stderr_preview: hook.stderr_preview,
                stdout_truncated: hook.stdout_truncated,
                stderr_truncated: hook.stderr_truncated,
                output_file: hook.output_file,
            }),
        ),
        None => (TuiMessageSource::User, None),
    };
    TuiChatMessage {
        role: value.role,
        content: value.content.into_iter().map(content_block).collect(),
        input_id: value.input_id.map(|id| id.as_str().to_string()),
        source,
        stop_hook,
    }
}
fn content_block(value: sdk::ContentBlock) -> TuiContentBlock {
    match value {
        sdk::ContentBlock::Text { text } => TuiContentBlock::Text { text },
        sdk::ContentBlock::Image {
            source,
            placeholder,
        } => match source {
            sdk::ImageSource::Base64 { media_type, data } => TuiContentBlock::Image {
                media_type,
                base64: data,
                placeholder,
            },
        },
        sdk::ContentBlock::ToolUse { id, name, input } => {
            TuiContentBlock::ToolUse { id, name, input }
        }
        sdk::ContentBlock::ToolResult {
            tool_use_id,
            content,
            is_error,
            text,
        } => TuiContentBlock::ToolResult {
            tool_use_id,
            content,
            is_error,
            text,
        },
        sdk::ContentBlock::Thinking {
            thinking,
            signature,
        } => TuiContentBlock::Thinking {
            thinking,
            signature,
        },
    }
}

fn hook_event(value: sdk::HookEventView) -> TuiHookEvent {
    TuiHookEvent {
        hook_name: value.hook_name,
        status: match value.status {
            sdk::HookEventStatus::Running => TuiHookStatus::Running,
            sdk::HookEventStatus::Succeeded => TuiHookStatus::Succeeded,
            sdk::HookEventStatus::Blocked => TuiHookStatus::Blocked,
            sdk::HookEventStatus::Failed => TuiHookStatus::Failed,
        },
        matcher: value.matcher,
        command: value.command,
        result: value.result.map(|result| TuiHookResult {
            exit_code: result.exit_code,
            stdout: result.stdout,
            stderr: result.stderr,
            decision: result.decision,
            reason: result.reason,
            additional_context: result.additional_context,
        }),
    }
}
fn hook_message(value: sdk::HookMessageView) -> TuiHookMessage {
    TuiHookMessage {
        point: value.point,
        source: value.source,
        execution_ordinal: value.execution_ordinal,
        attempt: value.attempt,
        kind: match value.kind {
            sdk::HookMessageKindView::AdditionalContext => TuiHookMessageKind::AdditionalContext,
            sdk::HookMessageKindView::SystemMessage => TuiHookMessageKind::SystemMessage,
        },
        text: value.text,
    }
}
fn agent_progress(value: sdk::AgentProgressEventView) -> TuiAgentProgress {
    TuiAgentProgress {
        sequence: value.sequence,
        kind: match value.kind {
            sdk::AgentProgressKindView::Started { role, model } => {
                TuiAgentProgressKind::Started { role, model }
            }
            sdk::AgentProgressKindView::Message { text } => TuiAgentProgressKind::Message { text },
            sdk::AgentProgressKindView::ToolCalls { calls } => TuiAgentProgressKind::ToolCalls {
                calls: calls
                    .into_iter()
                    .map(|call| TuiAgentToolCall {
                        id: call.id.as_str().to_string(),
                        name: call.name,
                        input: call.input,
                    })
                    .collect(),
            },
            sdk::AgentProgressKindView::ToolOutput { tool_name, text } => {
                TuiAgentProgressKind::ToolOutput { tool_name, text }
            }
        },
    }
}
fn config_field(value: sdk::ConfigField) -> TuiConfigField {
    match value {
        sdk::ConfigField::Model => TuiConfigField::Model,
        sdk::ConfigField::PermissionMode => TuiConfigField::PermissionMode,
        sdk::ConfigField::Memory => TuiConfigField::Memory,
    }
}
fn config_cause(value: sdk::ConfigChangeCause) -> TuiConfigChangeCause {
    match value {
        sdk::ConfigChangeCause::ClientUpdate => TuiConfigChangeCause::ClientUpdate,
        sdk::ConfigChangeCause::ProjectCommit => TuiConfigChangeCause::ProjectCommit,
        sdk::ConfigChangeCause::FileReload => TuiConfigChangeCause::FileReload,
    }
}
fn config_view(value: sdk::ConfigView) -> TuiConfigView {
    TuiConfigView {
        model_name: value.model_name,
        provider: value.provider,
        has_api_key: value.has_api_key,
        permission_mode: value.permission_mode,
        markdown: value.markdown,
        verbose: value.verbose,
        context_size: value.context_size,
        logging_level: value.logging_level,
    }
}
fn session_failure(value: sdk::SessionResumeFailureKind) -> TuiSessionResumeFailureKind {
    match value {
        sdk::SessionResumeFailureKind::NotFound => TuiSessionResumeFailureKind::NotFound,
        sdk::SessionResumeFailureKind::Corrupt => TuiSessionResumeFailureKind::Corrupt,
        sdk::SessionResumeFailureKind::Io => TuiSessionResumeFailureKind::Io,
    }
}
fn run_termination_reason(value: sdk::RunTerminationReason) -> TuiRunTerminationReason {
    match value {
        sdk::RunTerminationReason::UserExit => TuiRunTerminationReason::UserExit,
        sdk::RunTerminationReason::DoubleCtrlC => TuiRunTerminationReason::DoubleCtrlC,
        sdk::RunTerminationReason::QuitCommand => TuiRunTerminationReason::QuitCommand,
        sdk::RunTerminationReason::ProcessSignal => TuiRunTerminationReason::ProcessSignal,
        sdk::RunTerminationReason::SessionShutdown => TuiRunTerminationReason::SessionShutdown,
        sdk::RunTerminationReason::ParentStepCancelled => {
            TuiRunTerminationReason::ParentStepCancelled
        }
    }
}
fn reflection_record(value: sdk::ReflectionHistoryView) -> TuiReflectionRecord {
    TuiReflectionRecord {
        id: value.id,
        timestamp: value.timestamp,
        trigger: reflection_trigger(value.trigger),
        status: reflection_status(value.status),
        deviations: value.deviations,
        suggestions: value.suggestions,
        outdated: value.outdated,
        apply_status: reflection_apply_status(value.apply_status),
        error_category: value.error_category.map(reflection_error_category),
        token_usage: value
            .token_usage
            .map(|usage| (usage.input_tokens, usage.output_tokens)),
        duration_ms: value.duration_ms,
    }
}
fn reflection_trigger(value: sdk::ReflectionTriggerView) -> TuiReflectionTrigger {
    match value {
        sdk::ReflectionTriggerView::Interval => TuiReflectionTrigger::Interval,
        sdk::ReflectionTriggerView::PreCompact => TuiReflectionTrigger::PreCompact,
        sdk::ReflectionTriggerView::Manual => TuiReflectionTrigger::Manual,
    }
}
fn reflection_status(value: sdk::ReflectionStatusView) -> TuiReflectionStatus {
    match value {
        sdk::ReflectionStatusView::Running => TuiReflectionStatus::Running,
        sdk::ReflectionStatusView::Succeeded => TuiReflectionStatus::Succeeded,
        sdk::ReflectionStatusView::Failed => TuiReflectionStatus::Failed,
    }
}
fn reflection_apply_status(value: sdk::ReflectionApplyStatusView) -> TuiReflectionApplyStatus {
    match value {
        sdk::ReflectionApplyStatusView::NotApplied => TuiReflectionApplyStatus::NotApplied,
        sdk::ReflectionApplyStatusView::Applied => TuiReflectionApplyStatus::Applied,
        sdk::ReflectionApplyStatusView::PartiallyApplied => {
            TuiReflectionApplyStatus::PartiallyApplied
        }
    }
}
fn reflection_error_category(
    value: sdk::ReflectionErrorCategoryView,
) -> TuiReflectionErrorCategory {
    match value {
        sdk::ReflectionErrorCategoryView::LlmCall => TuiReflectionErrorCategory::LlmCall,
        sdk::ReflectionErrorCategoryView::EmptyResponse => {
            TuiReflectionErrorCategory::EmptyResponse
        }
        sdk::ReflectionErrorCategoryView::Parse => TuiReflectionErrorCategory::Parse,
        sdk::ReflectionErrorCategoryView::InvalidSuggestion => {
            TuiReflectionErrorCategory::InvalidSuggestion
        }
        sdk::ReflectionErrorCategoryView::Apply => TuiReflectionErrorCategory::Apply,
        sdk::ReflectionErrorCategoryView::History => TuiReflectionErrorCategory::History,
        sdk::ReflectionErrorCategoryView::Cancelled => TuiReflectionErrorCategory::Cancelled,
        sdk::ReflectionErrorCategoryView::TimedOut => TuiReflectionErrorCategory::TimedOut,
    }
}

#[cfg(test)]
#[path = "event_mapping_tests.rs"]
mod tests;
