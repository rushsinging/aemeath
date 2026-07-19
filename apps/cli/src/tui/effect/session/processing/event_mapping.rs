use crate::tui::app::event::{StatusContextUpdate, UiEvent};

pub(crate) fn sdk_event_to_ui_event(event: sdk::ChatEvent) -> UiEvent {
    match event {
        sdk::ChatEvent::Token { context, text } => UiEvent::Text {
            context: context.into(),
            text,
        },
        sdk::ChatEvent::Thinking { context, text } => UiEvent::Thinking {
            context: context.into(),
            text,
        },
        sdk::ChatEvent::BlockComplete { context, text } => UiEvent::BlockComplete {
            context: context.into(),
            text,
        },
        sdk::ChatEvent::ToolCallStart {
            context,
            id,
            provider_id,
            name,
            index,
        } => UiEvent::ToolCallStart {
            context: context.into(),
            id,
            provider_id,
            name,
            index,
        },
        sdk::ChatEvent::ToolCallUpdate {
            context,
            id,
            provider_id,
            name,
            index,
            arguments_delta,
            arguments,
            status,
        } => UiEvent::ToolCallUpdate {
            context: context.into(),
            id,
            provider_id,
            name,
            index,
            arguments_delta,
            arguments,
            status,
        },
        sdk::ChatEvent::ToolResult {
            context,
            id,
            provider_id,
            tool_name,
            output,
            content,
            is_error,
            images,
            ..
        } => UiEvent::ToolResult {
            context: context.into(),
            id,
            provider_id,
            tool_name,
            output,
            content,
            is_error,
            images,
        },
        sdk::ChatEvent::SystemMessage(msg) => UiEvent::SystemMessage(msg),
        sdk::ChatEvent::ModelStreamWaiting {
            context,
            elapsed_secs,
            phase,
        } => UiEvent::ModelStreamWaiting {
            context: context.into(),
            elapsed_secs,
            phase,
        },
        sdk::ChatEvent::ModelInvocationRetrying { attempt, delay, .. } => {
            UiEvent::SystemMessage(format!(
                "模型调用重试：第 {attempt} 次，等待 {} ms",
                delay.as_millis()
            ))
        }
        sdk::ChatEvent::Usage {
            input,
            output,
            last_input,
            elapsed_secs,
        } => UiEvent::Usage {
            input,
            output,
            last_input,
            elapsed_secs,
        },
        sdk::ChatEvent::TurnStarted { messages } => UiEvent::TurnStarted { messages },
        sdk::ChatEvent::MicrocompactDone {
            messages,
            cleared_count,
        } => UiEvent::MicrocompactDone {
            messages,
            cleared_count,
        },
        sdk::ChatEvent::StopHookBlocked { messages } => UiEvent::StopHookBlocked { messages },
        sdk::ChatEvent::PostToolExecutionSync { messages } => {
            UiEvent::PostToolExecutionSync { messages }
        }
        sdk::ChatEvent::ApiError { messages, error } => UiEvent::ApiError { messages, error },
        sdk::ChatEvent::CompactRollback { messages } => UiEvent::CompactRollback { messages },
        sdk::ChatEvent::CompactFinished { messages } => UiEvent::CompactFinished { messages },
        sdk::ChatEvent::UserMessagesAdopted { items, queued } => {
            UiEvent::UserMessagesAdopted { items, queued }
        }
        sdk::ChatEvent::UserMessagesQueued { queued } => UiEvent::UserMessagesQueued { queued },
        sdk::ChatEvent::Done { context } => UiEvent::Done {
            context: context.into(),
        },
        sdk::ChatEvent::DoneWithDurationMs {
            context,
            duration_ms,
        } => UiEvent::DoneWithDuration {
            context: context.into(),
            duration: std::time::Duration::from_millis(duration_ms),
        },
        sdk::ChatEvent::RunStarted { .. }
        | sdk::ChatEvent::RunStepStarted { .. }
        | sdk::ChatEvent::RunStepCompleted { .. }
        | sdk::ChatEvent::RunStepCancellationRequested { .. }
        | sdk::ChatEvent::RunStepFinalizationStarted { .. }
        | sdk::ChatEvent::RunStepCancelled { .. }
        | sdk::ChatEvent::RunDrainingInput { .. }
        | sdk::ChatEvent::RunTerminationRequested { .. }
        | sdk::ChatEvent::RunTerminated { .. }
        | sdk::ChatEvent::RunCompleted { .. }
        | sdk::ChatEvent::RunFailed { .. }
        | sdk::ChatEvent::RunStuckDetected { .. }
        | sdk::ChatEvent::RunTransitioned { .. }
        | sdk::ChatEvent::RunAwaitingUser { .. }
        | sdk::ChatEvent::RunResumed { .. }
        | sdk::ChatEvent::InteractionRequested { .. }
        | sdk::ChatEvent::RunCancelling { .. } => UiEvent::SystemMessage(String::new()),
        sdk::ChatEvent::RunCancelled { .. } => UiEvent::RunCancelled,
        sdk::ChatEvent::Cancelled { context } => UiEvent::Cancelled {
            context: context.into(),
        },
        sdk::ChatEvent::LiveTps(tps) => UiEvent::LiveTps(tps),
        sdk::ChatEvent::CurrentTurnChanged(turn) | sdk::ChatEvent::TurnChanged(turn) => {
            UiEvent::CurrentTurnChanged(turn)
        }
        sdk::ChatEvent::HookEvent(event) => UiEvent::HookEvent(event),
        // Structured hook message has no dedicated UiEvent yet; intentionally ignored.
        sdk::ChatEvent::HookMessage(_) => UiEvent::SystemMessage(String::new()),
        sdk::ChatEvent::AskUserBatch { items, reply_tx } => {
            UiEvent::AskUserBatch { items, reply_tx }
        }
        sdk::ChatEvent::AgentProgress {
            context,
            tool_id,
            event,
        } => UiEvent::AgentProgress {
            context: context.into(),
            tool_id,
            event,
        },
        sdk::ChatEvent::WorkingDirectoryChanged {
            path_base,
            workspace_root,
            workspace,
        } => UiEvent::WorkingDirectoryChanged(StatusContextUpdate {
            path_base: crate::tui::app::display_status_path(std::path::Path::new(&path_base)),
            workspace_root: crate::tui::app::display_status_path(std::path::Path::new(
                &workspace_root,
            )),
            branch: crate::tui::app::git_branch_for(std::path::Path::new(&workspace_root)),
            kind: crate::tui::app::worktree_kind_for(std::path::Path::new(&workspace_root)),
            raw_path_base: std::path::PathBuf::from(path_base),
            raw_workspace_root: std::path::PathBuf::from(workspace_root),
            workspace,
        }),
        sdk::ChatEvent::ConfigChanged { event } => UiEvent::SystemMessage(format!(
            "[config changed] fields: {:?}",
            event.changed_fields
        )),
        sdk::ChatEvent::ConfigReloaded { changed_keys } => {
            let keys_str = changed_keys.join(", ");
            UiEvent::SystemMessage(format!("[config reloaded] changed: {}", keys_str))
        }
        sdk::ChatEvent::SessionReset => UiEvent::SessionReset,
        sdk::ChatEvent::UserMessagesWithdrawn { texts } => UiEvent::UserMessagesWithdrawn(texts),
        sdk::ChatEvent::GraphPhaseChanged { node, .. } => UiEvent::GraphPhaseChanged { node },
        sdk::ChatEvent::CompactProgress {
            stage,
            current,
            total,
        } => UiEvent::CompactProgress {
            stage,
            current,
            total,
        },
        sdk::ChatEvent::ModelSwitched { result } => UiEvent::ModelSwitched { result },
        sdk::ChatEvent::ThinkingChanged { enabled } => UiEvent::ThinkingChanged { enabled },
        sdk::ChatEvent::ContextEstimated {
            estimate,
            message_count,
        } => UiEvent::ContextEstimated {
            estimate,
            message_count,
        },
        sdk::ChatEvent::CommandResultText { text, is_error } => {
            UiEvent::CommandResultText { text, is_error }
        }
        sdk::ChatEvent::SessionResumed {
            messages,
            session_id,
            created_at,
        } => UiEvent::SessionResumed {
            messages,
            session_id,
            created_at,
        },
        sdk::ChatEvent::SessionResumeFailed { kind, id, message } => {
            UiEvent::SessionResumeFailed { kind, id, message }
        }
        sdk::ChatEvent::ReflectionHistory { records } => UiEvent::ReflectionHistory { records },
        sdk::ChatEvent::Result(result) => UiEvent::SystemMessage(result.text),
        // These list/project/cost events have no dedicated UiEvent yet and are intentionally ignored.
        sdk::ChatEvent::ModelList { .. }
        | sdk::ChatEvent::ReminderList { .. }
        | sdk::ChatEvent::SessionList { .. }
        | sdk::ChatEvent::ProjectInfo { .. }
        | sdk::ChatEvent::CostUpdate { .. } => UiEvent::SystemMessage(String::new()),
        sdk::ChatEvent::TasksSnapshot { tasks } => UiEvent::TaskStatusChanged(*tasks),
    }
}
