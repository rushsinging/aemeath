use super::runtime_status::{TuiContextBudget, TuiContextDecisionSource, TuiRuntimeStatus};
use super::runtime_view::{
    TuiChatMessage, TuiContentBlock, TuiHookNotice, TuiHookNoticeKind, TuiMessageSource,
    TuiSkillRequestMetadata, TuiTaskBatch, TuiTaskBatchStatus, TuiTaskItem, TuiTaskItemStatus,
    TuiTaskPriority, TuiTaskState, TuiToolResultImage,
};
use super::tui_runtime_event::*;
use crate::tui::model::conversation::interaction::{UiInteractionRequestId, UiRunId, UiRunStepId};

pub(crate) enum SdkEventMapping {
    Runtime(TuiRuntimeEvent),
}

pub(crate) fn sdk_event_to_tui_event(event: sdk::ChatEvent) -> SdkEventMapping {
    use sdk::ChatEvent;

    let runtime = match event {
        ChatEvent::ActivityChanged { kind, activity } => TuiRuntimeEvent::ActivityChanged {
            kind: activity_change_kind(kind),
            activity: activity_observation(activity),
        },
        ChatEvent::ActivitySnapshot(snapshot) => {
            TuiRuntimeEvent::ActivitySnapshot(activity_snapshot(snapshot))
        }
        ChatEvent::SkillsUpdated { event } => TuiRuntimeEvent::SkillsUpdated {
            revision: event.revision,
            skills: event
                .skills
                .into_iter()
                .map(
                    |skill| crate::tui::adapter::tui_runtime_event::TuiSkillView {
                        name: skill.name,
                        aliases: skill.aliases,
                        slash_command: skill.slash_command,
                        slash_aliases: skill.slash_aliases,
                        description: skill.description,
                        argument_hint: skill.argument_hint,
                    },
                )
                .collect(),
            slash_routes: event
                .slash_routes
                .into_iter()
                .map(
                    |route| crate::tui::adapter::tui_runtime_event::TuiSkillSlashRoute {
                        skill: route.skill,
                        slash_command: route.slash_command,
                        aliases: route.aliases,
                        argument_hint: route.argument_hint,
                    },
                )
                .collect(),
        },
        ChatEvent::AssistantTextDelta { context, delta } => TuiRuntimeEvent::AssistantTextDelta {
            context: turn_context(context),
            delta,
        },
        ChatEvent::ThinkingDelta { context, delta } => TuiRuntimeEvent::ThinkingDelta {
            context: turn_context(context),
            delta,
        },
        ChatEvent::Token { context, text } => TuiRuntimeEvent::AssistantTextDelta {
            context: turn_context(context),
            delta: text,
        },
        ChatEvent::Thinking { context, text } => TuiRuntimeEvent::ThinkingDelta {
            context: turn_context(context),
            delta: text,
        },
        ChatEvent::BlockComplete { context, text } => TuiRuntimeEvent::BlockComplete {
            context: turn_context(context),
            text,
        },
        ChatEvent::ToolCallStarted {
            context,
            id,
            provider_id,
            name,
            index,
        }
        | ChatEvent::ToolCallStart {
            context,
            id,
            provider_id,
            name,
            index,
        } => TuiRuntimeEvent::ToolCallStarted {
            context: turn_context(context),
            id: id.as_str().to_string(),
            provider_id,
            name,
            index,
        },
        ChatEvent::ToolCallArgumentsDelta {
            context,
            id,
            provider_id,
            name,
            index,
            delta,
        } => TuiRuntimeEvent::ToolCallArgumentsDelta {
            context: turn_context(context),
            id: id.as_str().to_string(),
            provider_id,
            name,
            index,
            delta,
        },
        ChatEvent::ToolCallStateChanged {
            context,
            id,
            provider_id,
            name,
            index,
            arguments,
            status,
        } => TuiRuntimeEvent::ToolCallStateChanged {
            context: turn_context(context),
            id: id.as_str().to_string(),
            provider_id,
            name,
            index,
            arguments,
            status: tool_status(status),
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
        } => match arguments_delta {
            Some(delta) => TuiRuntimeEvent::ToolCallArgumentsDelta {
                context: turn_context(context),
                id: id.as_str().to_string(),
                provider_id,
                name,
                index,
                delta,
            },
            None => TuiRuntimeEvent::ToolCallStateChanged {
                context: turn_context(context),
                id: id.as_str().to_string(),
                provider_id,
                name,
                index,
                arguments,
                status: tool_status(status),
            },
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
        ChatEvent::MicrocompactCompleted {
            messages,
            cleared_count,
        }
        | ChatEvent::MicrocompactDone {
            messages,
            cleared_count,
        } => TuiRuntimeEvent::MicrocompactCompleted {
            messages: messages.into_iter().map(chat_message).collect(),
            cleared_count,
        },
        ChatEvent::SessionMessageStateChanged {
            message_count,
            revision,
        } => TuiRuntimeEvent::SessionMessageStateChanged {
            message_count,
            revision,
        },
        ChatEvent::HookNotice { notice } => TuiRuntimeEvent::HookNotice(hook_notice(notice)),
        ChatEvent::ApiError { messages, error } => TuiRuntimeEvent::ApiError {
            messages: messages.into_iter().map(chat_message).collect(),
            error,
        },
        ChatEvent::CompactOperationRolledBack { messages }
        | ChatEvent::CompactRollback { messages } => TuiRuntimeEvent::CompactOperationRolledBack {
            messages: messages.into_iter().map(chat_message).collect(),
        },
        ChatEvent::CompactOperationCompleted { messages, notice }
        | ChatEvent::CompactFinished { messages, notice } => {
            TuiRuntimeEvent::CompactOperationCompleted {
                messages: messages.into_iter().map(chat_message).collect(),
                notice,
            }
        }
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
        ChatEvent::RunTransitioned { .. } => TuiRuntimeEvent::Noop,
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
            terminal,
        } => run_step_event(
            run_id,
            parent_run_id,
            step_id,
            TuiRunStepEvent::Cancelled {
                terminal: match terminal {
                    sdk::RunStepCancellationTerminal::Cancelled => {
                        TuiRunStepCancellationTerminal::Cancelled
                    }
                    sdk::RunStepCancellationTerminal::CancellationUnconfirmed => {
                        TuiRunStepCancellationTerminal::CancellationUnconfirmed
                    }
                },
            },
        ),
        ChatEvent::InteractionRequested { request } => {
            TuiRuntimeEvent::InteractionRequested(interaction_request(request))
        }
        ChatEvent::Cancelled {
            context,
            duration_ms,
        } => TuiRuntimeEvent::Cancelled {
            context: turn_context(context),
            duration_ms,
        },
        ChatEvent::LiveTps(tps) => TuiRuntimeEvent::LiveTps(tps),
        ChatEvent::RunChanged(turn) | ChatEvent::CurrentRunChanged(turn) => {
            TuiRuntimeEvent::RunChanged(turn)
        }
        ChatEvent::AgentProgress {
            source_context,
            attachment_context,
            tool_id,
            event,
        } => TuiRuntimeEvent::AgentProgress {
            source_context: turn_context(source_context),
            attachment_context: turn_context(attachment_context),
            tool_id: tool_id.as_str().to_string(),
            event: agent_progress(event),
        },
        ChatEvent::ToolOutputDelta {
            context,
            tool_id,
            delta,
        } => TuiRuntimeEvent::ToolOutputDelta {
            context: turn_context(context),
            tool_id: tool_id.as_str().to_string(),
            delta,
        },
        ChatEvent::ToolProgress {
            context,
            tool_id,
            event,
        } => TuiRuntimeEvent::ToolOutputDelta {
            context: turn_context(context),
            tool_id: tool_id.as_str().to_string(),
            delta: event.text,
        },
        ChatEvent::ChildRunActivity { event } => {
            TuiRuntimeEvent::ChildRunActivity(child_run_activity(event))
        }
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
            view: config_view(event.view),
        },
        ChatEvent::SessionReset => TuiRuntimeEvent::SessionReset,
        ChatEvent::UserMessagesWithdrawn { texts } => {
            TuiRuntimeEvent::UserMessagesWithdrawn { texts }
        }
        ChatEvent::Result(result) => TuiRuntimeEvent::CommandResultText {
            text: result.text,
            is_error: false,
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
            display_history,
            session_id,
            created_at,
            compacted,
        } => TuiRuntimeEvent::SessionResumed {
            steps: steps
                .into_iter()
                .map(|step| super::runtime_view::TuiResumedSessionStep {
                    run_id: step.run_id,
                    step_id: step.step_id,
                    messages: step.messages.into_iter().map(chat_message).collect(),
                    finalize_cause: step.finalize_cause.map(|cause| match cause {
                        sdk::ResumedStepFinalizeCause::Completed => {
                            super::runtime_view::TuiResumedStepFinalizeCause::Completed
                        }
                        sdk::ResumedStepFinalizeCause::UserCancelledStep => {
                            super::runtime_view::TuiResumedStepFinalizeCause::UserCancelledStep
                        }
                        sdk::ResumedStepFinalizeCause::RunTerminated => {
                            super::runtime_view::TuiResumedStepFinalizeCause::RunTerminated
                        }
                    }),
                    duration_ms: step.duration_ms,
                })
                .collect(),
            display_history: display_history.map(tui_display_history_index),
            session_id,
            created_at,
            compacted,
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
        ChatEvent::RuntimeStatusChanged { status } => TuiRuntimeEvent::RuntimeStatusChanged {
            status: Box::new(TuiRuntimeStatus {
                session_id: status.session_id,
                revision: status.revision,
                heartbeat_sequence: status.heartbeat_sequence,
                context_budget: TuiContextBudget {
                    context_size: status.context_budget.context_size,
                    effective_window: status.context_budget.effective_window,
                    decision_token_count: status.context_budget.decision_token_count,
                    threshold: status.context_budget.threshold,
                    usage_permille: status.context_budget.usage_permille,
                    compaction_needed: status.context_budget.compaction_needed,
                    source: match status.context_budget.source {
                        sdk::ContextDecisionSourceView::ActualProviderUsage => {
                            TuiContextDecisionSource::ActualProviderUsage
                        }
                        sdk::ContextDecisionSourceView::HeuristicFallback => {
                            TuiContextDecisionSource::HeuristicFallback
                        }
                        sdk::ContextDecisionSourceView::Manual => TuiContextDecisionSource::Manual,
                    },
                },
            }),
        },
        ChatEvent::TaskStateChanged { state } => TuiRuntimeEvent::TaskStateChanged {
            state: Box::new(TuiTaskState {
                session_id: state.session_id,
                revision: state.revision,
                current_batch: state.current_batch.map(|batch| TuiTaskBatch {
                    id: batch.id,
                    summary: batch.summary,
                    status: match batch.status {
                        sdk::TaskBatchStatusView::Active => TuiTaskBatchStatus::Active,
                        sdk::TaskBatchStatusView::Paused => TuiTaskBatchStatus::Paused,
                        sdk::TaskBatchStatusView::Archived => TuiTaskBatchStatus::Archived,
                    },
                }),
                total: state.total,
                completed: state.completed,
                in_progress: state.in_progress,
                items: state
                    .items
                    .into_iter()
                    .map(|item| TuiTaskItem {
                        id: item.id,
                        sequence: item.sequence,
                        subject: item.subject,
                        status: match item.status {
                            sdk::TaskItemStatusView::Pending => TuiTaskItemStatus::Pending,
                            sdk::TaskItemStatusView::InProgress => TuiTaskItemStatus::InProgress,
                            sdk::TaskItemStatusView::Completed => TuiTaskItemStatus::Completed,
                        },
                        priority: match item.priority {
                            sdk::TaskPriorityView::Low => TuiTaskPriority::Low,
                            sdk::TaskPriorityView::Normal => TuiTaskPriority::Normal,
                            sdk::TaskPriorityView::High => TuiTaskPriority::High,
                            sdk::TaskPriorityView::Urgent => TuiTaskPriority::Urgent,
                        },
                        blocked_by_sequences: item.blocked_by_sequences,
                    })
                    .collect(),
                hidden_count: state.hidden_count,
            }),
        },
        ChatEvent::CostUpdate { cost } => TuiRuntimeEvent::CostUpdate {
            input_tokens: cost.input_tokens,
            output_tokens: cost.output_tokens,
            cost_usd: cost.cost_usd,
        },
    };
    SdkEventMapping::Runtime(runtime)
}

fn activity_change_kind(value: sdk::ActivityChangeKind) -> TuiActivityChangeKind {
    match value {
        sdk::ActivityChangeKind::Started => TuiActivityChangeKind::Started,
        sdk::ActivityChangeKind::Updated => TuiActivityChangeKind::Updated,
        sdk::ActivityChangeKind::Finished => TuiActivityChangeKind::Finished,
    }
}

fn activity_snapshot(value: sdk::ActivitySnapshotView) -> TuiActivitySnapshot {
    TuiActivitySnapshot {
        run_id: run_id(value.run_id),
        revision: value.revision,
        activities: value
            .activities
            .into_iter()
            .map(activity_observation)
            .collect(),
    }
}

fn activity_observation(value: sdk::ActivityView) -> TuiActivityObservation {
    TuiActivityObservation {
        id: UiActivityId::from(value.id.as_str()),
        run_id: run_id(value.run_id),
        run_step_id: value
            .run_step_id
            .as_ref()
            .map(|id| UiRunStepId::from(id.as_str())),
        parent_activity_id: value
            .parent_activity_id
            .as_ref()
            .map(|id| UiActivityId::from(id.as_str())),
        source: activity_source(value.source),
        kind: activity_kind(value.kind),
        state: activity_state(value.state),
        detail: activity_detail(value.detail),
        audience: activity_audience(value.audience),
        revision: value.revision,
        timing: TuiActivityTiming {
            total_elapsed_ms: value.timing.total_elapsed_ms,
            active_elapsed_ms: value.timing.active_elapsed_ms,
            state_elapsed_ms: value.timing.state_elapsed_ms,
            started_at_unix_ms: value.timing.started_at_unix_ms,
            finished_at_unix_ms: value.timing.finished_at_unix_ms,
        },
    }
}

fn activity_source(value: sdk::ActivitySourceView) -> TuiActivitySource {
    match value {
        sdk::ActivitySourceView::Run => TuiActivitySource::Run,
        sdk::ActivitySourceView::RunStep(id) => {
            TuiActivitySource::RunStep(UiRunStepId::from(id.as_str()))
        }
        sdk::ActivitySourceView::ModelInvocation(id) => {
            TuiActivitySource::ModelInvocation(id.as_str().to_string())
        }
        sdk::ActivitySourceView::ToolCall(id) => {
            TuiActivitySource::ToolCall(id.as_str().to_string())
        }
        sdk::ActivitySourceView::HookDispatch(id) => {
            TuiActivitySource::HookDispatch(UiActivityId::from(id.as_str()))
        }
        sdk::ActivitySourceView::Compaction(id) => {
            TuiActivitySource::Compaction(UiActivityId::from(id.as_str()))
        }
        sdk::ActivitySourceView::Interaction(id) => {
            TuiActivitySource::Interaction(id.as_str().to_string())
        }
        sdk::ActivitySourceView::ChildRun(id) => {
            TuiActivitySource::ChildRun(UiRunId::from(id.as_str()))
        }
    }
}

fn activity_kind(value: sdk::ActivityKindView) -> TuiActivityKind {
    match value {
        sdk::ActivityKindView::Run => TuiActivityKind::Run,
        sdk::ActivityKindView::RunPhase(phase) => TuiActivityKind::RunPhase(run_phase(phase)),
        sdk::ActivityKindView::ModelInvocation => TuiActivityKind::ModelInvocation,
        sdk::ActivityKindView::ToolCall => TuiActivityKind::ToolCall,
        sdk::ActivityKindView::HookDispatch => TuiActivityKind::HookDispatch,
        sdk::ActivityKindView::Compaction => TuiActivityKind::Compaction,
        sdk::ActivityKindView::Interaction => TuiActivityKind::Interaction,
        sdk::ActivityKindView::ChildRun => TuiActivityKind::ChildRun,
    }
}

fn activity_state(value: sdk::ActivityStateView) -> TuiActivityState {
    match value {
        sdk::ActivityStateView::Running => TuiActivityState::Running,
        sdk::ActivityStateView::Waiting => TuiActivityState::Waiting,
        sdk::ActivityStateView::Succeeded => TuiActivityState::Succeeded,
        sdk::ActivityStateView::Failed => TuiActivityState::Failed,
        sdk::ActivityStateView::Cancelled => TuiActivityState::Cancelled,
        sdk::ActivityStateView::Terminated => TuiActivityState::Terminated,
    }
}

fn activity_audience(value: sdk::ActivityAudienceView) -> TuiActivityAudience {
    match value {
        sdk::ActivityAudienceView::User => TuiActivityAudience::User,
        sdk::ActivityAudienceView::Operational => TuiActivityAudience::Operational,
        sdk::ActivityAudienceView::Diagnostic => TuiActivityAudience::Diagnostic,
    }
}

fn run_phase(value: sdk::RunPhaseKindView) -> TuiRunPhaseKind {
    match value {
        sdk::RunPhaseKindView::DrainingInput => TuiRunPhaseKind::DrainingInput,
        sdk::RunPhaseKindView::PreparingContext => TuiRunPhaseKind::PreparingContext,
        sdk::RunPhaseKindView::ApplyingResponse => TuiRunPhaseKind::ApplyingResponse,
        sdk::RunPhaseKindView::AwaitingToolApproval => TuiRunPhaseKind::AwaitingToolApproval,
        sdk::RunPhaseKindView::ExecutingTools => TuiRunPhaseKind::ExecutingTools,
        sdk::RunPhaseKindView::FinalizingStep => TuiRunPhaseKind::FinalizingStep,
        sdk::RunPhaseKindView::CancellingStep => TuiRunPhaseKind::CancellingStep,
        sdk::RunPhaseKindView::Terminating => TuiRunPhaseKind::Terminating,
    }
}

fn activity_detail(value: sdk::ActivityDetailView) -> TuiActivityDetail {
    match value {
        sdk::ActivityDetailView::Run { purpose } => TuiActivityDetail::Run {
            purpose: match purpose {
                sdk::RunPurposeView::Main => TuiRunPurpose::Main,
                sdk::RunPurposeView::Derived => TuiRunPurpose::Derived,
            },
        },
        sdk::ActivityDetailView::Phase { phase } => TuiActivityDetail::Phase {
            phase: run_phase(phase),
        },
        sdk::ActivityDetailView::Model {
            model,
            attempt,
            stream,
        } => TuiActivityDetail::Model {
            model,
            attempt,
            stream: match stream {
                sdk::ModelStreamStateView::Invoking => TuiModelStreamState::Invoking,
                sdk::ModelStreamStateView::WaitingForFirstToken => {
                    TuiModelStreamState::WaitingForFirstToken
                }
                sdk::ModelStreamStateView::Streaming => TuiModelStreamState::Streaming,
                sdk::ModelStreamStateView::Retrying => TuiModelStreamState::Retrying,
            },
        },
        sdk::ActivityDetailView::Tool {
            name,
            summary,
            parallel_count,
        } => TuiActivityDetail::Tool {
            name,
            summary,
            parallel_count,
        },
        sdk::ActivityDetailView::Hook {
            point,
            script,
            attempt,
        } => TuiActivityDetail::Hook {
            point: hook_point(point),
            script,
            attempt,
        },
        sdk::ActivityDetailView::Compact { stage, work } => TuiActivityDetail::Compact {
            stage: match stage {
                sdk::CompactStageView::Preparing => TuiCompactStage::Preparing,
                sdk::CompactStageView::Generating => TuiCompactStage::Generating,
                sdk::CompactStageView::Mapping => TuiCompactStage::Mapping,
                sdk::CompactStageView::Reducing => TuiCompactStage::Reducing,
                sdk::CompactStageView::Refreshing => TuiCompactStage::Refreshing,
                sdk::CompactStageView::Finalizing => TuiCompactStage::Finalizing,
            },
            work: match work {
                sdk::CompactWorkView::Indeterminate => TuiCompactWork::Indeterminate,
                sdk::CompactWorkView::Determinate { completed, total } => {
                    TuiCompactWork::Determinate { completed, total }
                }
            },
        },
        sdk::ActivityDetailView::Interaction { kind } => TuiActivityDetail::Interaction {
            kind: match kind {
                sdk::InteractionKindView::ToolApproval => TuiInteractionKind::ToolApproval,
                sdk::InteractionKindView::UserQuestion => TuiInteractionKind::UserQuestion,
                sdk::InteractionKindView::PlanApproval => TuiInteractionKind::PlanApproval,
                sdk::InteractionKindView::StuckDiagnostic => TuiInteractionKind::StuckDiagnostic,
            },
        },
        sdk::ActivityDetailView::ChildRun { role, model } => {
            TuiActivityDetail::ChildRun { role, model }
        }
    }
}

fn hook_point(value: sdk::HookPointView) -> TuiHookPoint {
    match value {
        sdk::HookPointView::PreToolUse => TuiHookPoint::PreToolUse,
        sdk::HookPointView::UserPromptSubmit => TuiHookPoint::UserPromptSubmit,
        sdk::HookPointView::PreCompact => TuiHookPoint::PreCompact,
        sdk::HookPointView::PermissionRequest => TuiHookPoint::PermissionRequest,
        sdk::HookPointView::Elicitation => TuiHookPoint::Elicitation,
        sdk::HookPointView::UserPromptExpansion => TuiHookPoint::UserPromptExpansion,
        sdk::HookPointView::Stop => TuiHookPoint::Stop,
        sdk::HookPointView::PostToolUse => TuiHookPoint::PostToolUse,
        sdk::HookPointView::PostToolUseFailure => TuiHookPoint::PostToolUseFailure,
        sdk::HookPointView::PostCompact => TuiHookPoint::PostCompact,
        sdk::HookPointView::PostToolBatch => TuiHookPoint::PostToolBatch,
        sdk::HookPointView::ElicitationResult => TuiHookPoint::ElicitationResult,
        sdk::HookPointView::SessionStart => TuiHookPoint::SessionStart,
        sdk::HookPointView::SessionEnd => TuiHookPoint::SessionEnd,
        sdk::HookPointView::SubRunStart => TuiHookPoint::SubRunStart,
        sdk::HookPointView::SubRunStop => TuiHookPoint::SubRunStop,
        sdk::HookPointView::TaskCreated => TuiHookPoint::TaskCreated,
        sdk::HookPointView::TaskCompleted => TuiHookPoint::TaskCompleted,
        sdk::HookPointView::Notification => TuiHookPoint::Notification,
        sdk::HookPointView::InstructionsLoaded => TuiHookPoint::InstructionsLoaded,
        sdk::HookPointView::StopFailure => TuiHookPoint::StopFailure,
        sdk::HookPointView::PermissionDenied => TuiHookPoint::PermissionDenied,
        sdk::HookPointView::ConfigChange => TuiHookPoint::ConfigChange,
        sdk::HookPointView::CwdChanged => TuiHookPoint::CwdChanged,
        sdk::HookPointView::FileChanged => TuiHookPoint::FileChanged,
        sdk::HookPointView::TeammateIdle => TuiHookPoint::TeammateIdle,
    }
}

fn turn_context(value: sdk::ChatEventContext) -> TuiRunContext {
    TuiRunContext {
        chat_id: value.chat_id.as_str().to_string(),
        run_id: value.run_id.as_str().to_string(),
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
        tool_call_id: value.tool_call_id,
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

fn hook_notice(notice: sdk::HookNoticeView) -> TuiHookNotice {
    TuiHookNotice {
        point: notice.point,
        kind: match notice.kind {
            sdk::HookNoticeKindView::Blocked => TuiHookNoticeKind::Blocked,
            sdk::HookNoticeKindView::Failed => TuiHookNoticeKind::Failed,
            sdk::HookNoticeKindView::Info => TuiHookNoticeKind::Info,
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
    }
}

pub(crate) fn chat_message(value: sdk::ChatMessage) -> TuiChatMessage {
    let metadata_present = value.metadata.is_some();
    let (source, hook_notice, skill_request) = match value.metadata {
        Some(metadata) => (
            match metadata.source {
                sdk::ChatMessageSource::User => TuiMessageSource::User,
                sdk::ChatMessageSource::SystemGenerated => TuiMessageSource::SystemGenerated,
                sdk::ChatMessageSource::Hook => TuiMessageSource::Hook,
                sdk::ChatMessageSource::SkillRequest => TuiMessageSource::SkillRequest,
            },
            metadata.hook_notice.map(hook_notice),
            metadata
                .skill_request
                .map(|request| TuiSkillRequestMetadata {
                    skill: request.skill,
                    arguments: request.arguments,
                    raw_input: request.raw_input,
                }),
        ),
        None => (TuiMessageSource::User, None, None),
    };
    crate::tui::log_debug!(
        "skill_request boundary=sdk_to_tui source={:?} role={} content_blocks={} content_text_len={} metadata_present={} skill_metadata_present={} input_id_present={}",
        source,
        value.role,
        value.content.len(),
        value
            .content
            .iter()
            .map(|block| match block {
                sdk::ContentBlock::Text { text } => text.len(),
                _ => 0,
            })
            .sum::<usize>(),
        metadata_present,        skill_request.is_some(),
        value.input_id.is_some()
    );
    TuiChatMessage {
        role: value.role,
        content: value.content.into_iter().map(content_block).collect(),
        input_id: value.input_id.map(|id| id.as_str().to_string()),
        source,
        hook_notice,
        skill_request,
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

fn child_run_activity(value: sdk::ChildRunActivityEventView) -> TuiChildRunActivity {
    TuiChildRunActivity {
        identity: TuiChildRunIdentity {
            agent_id: value.identity.agent_id.as_str().to_string(),
            run_id: UiRunId::from(value.identity.run_id.as_str()),
            parent_run_id: UiRunId::from(value.identity.parent_run_id.as_str()),
            spawned_by_tool_call_id: value.identity.spawned_by_tool_call_id.as_str().to_string(),
        },
        sequence: value.sequence,
        kind: match value.kind {
            sdk::ChildRunActivityKindView::Text { text } => TuiChildRunActivityKind::Text { text },
            sdk::ChildRunActivityKindView::Thinking { text } => {
                TuiChildRunActivityKind::Thinking { text }
            }
            sdk::ChildRunActivityKindView::ToolCall { id, name, input } => {
                TuiChildRunActivityKind::ToolCall {
                    id: id.as_str().to_string(),
                    name,
                    input,
                }
            }
            sdk::ChildRunActivityKindView::ToolOutput { tool_name, text } => {
                TuiChildRunActivityKind::ToolOutput { tool_name, text }
            }
            sdk::ChildRunActivityKindView::ToolResult {
                tool_call_id,
                tool_name,
                output,
                content,
                is_error,
            } => TuiChildRunActivityKind::ToolResult {
                tool_call_id: tool_call_id.as_str().to_string(),
                tool_name,
                output,
                content,
                is_error,
            },
            sdk::ChildRunActivityKindView::Terminal { outcome } => {
                TuiChildRunActivityKind::Terminal {
                    outcome: match outcome {
                        sdk::ChildRunTerminalOutcomeView::Completed => {
                            TuiChildRunTerminalOutcome::Completed
                        }
                        sdk::ChildRunTerminalOutcomeView::Failed { error } => {
                            TuiChildRunTerminalOutcome::Failed { error }
                        }
                        sdk::ChildRunTerminalOutcomeView::Cancelled => {
                            TuiChildRunTerminalOutcome::Cancelled
                        }
                    },
                }
            }
        },
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
    let markdown_spacing =
        crate::tui::view_model::markdown_spacing::MarkdownSpacingPolicy::from(&value);
    TuiConfigView {
        model_name: value.model_name,
        provider: value.provider,
        has_api_key: value.has_api_key,
        permission_mode: value.permission_mode,
        markdown: value.markdown,
        verbose: value.verbose,
        markdown_spacing,
        context_size: value.context_size,
        logging_level: value.logging_level,
    }
}
pub(crate) fn tui_display_history_window(
    window: sdk::DisplayHistoryWindow,
) -> super::runtime_view::TuiDisplayHistoryWindow {
    super::runtime_view::TuiDisplayHistoryWindow {
        session_id: window.session_id,
        generation_revision: window.generation_revision,
        steps: window
            .steps
            .into_iter()
            .map(|step| super::runtime_view::TuiResumedSessionStep {
                run_id: step.run_id,
                step_id: step.step_id,
                messages: step.messages.into_iter().map(chat_message).collect(),
                finalize_cause: step.finalize_cause.map(|cause| match cause {
                    sdk::ResumedStepFinalizeCause::Completed => {
                        super::runtime_view::TuiResumedStepFinalizeCause::Completed
                    }
                    sdk::ResumedStepFinalizeCause::UserCancelledStep => {
                        super::runtime_view::TuiResumedStepFinalizeCause::UserCancelledStep
                    }
                    sdk::ResumedStepFinalizeCause::RunTerminated => {
                        super::runtime_view::TuiResumedStepFinalizeCause::RunTerminated
                    }
                }),
                duration_ms: step.duration_ms,
            })
            .collect(),
    }
}

pub(crate) fn tui_display_history_index(
    index: sdk::DisplayHistoryIndex,
) -> super::runtime_view::TuiDisplayHistoryIndex {
    super::runtime_view::TuiDisplayHistoryIndex {
        session_id: index.session_id,
        generation_revision: index.generation_revision,
        steps: index
            .steps
            .into_iter()
            .map(|step| super::runtime_view::TuiDisplayHistoryStepReference {
                run_id: step.run_id,
                step_id: step.step_id,
                member_name: step.member_name,
                estimated_lines: step.estimated_lines,
                user_input_history: step.user_input_history,
                finalize_cause: step.finalize_cause.map(|cause| match cause {
                    sdk::ResumedStepFinalizeCause::Completed => {
                        super::runtime_view::TuiResumedStepFinalizeCause::Completed
                    }
                    sdk::ResumedStepFinalizeCause::UserCancelledStep => {
                        super::runtime_view::TuiResumedStepFinalizeCause::UserCancelledStep
                    }
                    sdk::ResumedStepFinalizeCause::RunTerminated => {
                        super::runtime_view::TuiResumedStepFinalizeCause::RunTerminated
                    }
                }),
                duration_ms: step.duration_ms,
            })
            .collect(),
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
