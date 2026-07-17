use crate::tui::app::event::UiEvent;

pub(crate) fn log_sdk_event(event: &sdk::ChatEvent, stage: &'static str) {
    match event {
        sdk::ChatEvent::Token { context, text } => crate::tui::log_trace!(
            "{} token chat_id={} turn_id={} text_len={}",
            stage,
            context.chat_id,
            context.turn_id,
            text.len()
        ),
        sdk::ChatEvent::Thinking { context, text } => crate::tui::log_trace!(
            "{} thinking chat_id={} turn_id={} text_len={}",
            stage,
            context.chat_id,
            context.turn_id,
            text.len()
        ),
        sdk::ChatEvent::BlockComplete { context, text } => crate::tui::log_trace!(
            "{} block_complete chat_id={} turn_id={} text_len={}",
            stage,
            context.chat_id,
            context.turn_id,
            text.len()
        ),
        sdk::ChatEvent::ToolCallStart {
            context,
            id,
            provider_id,
            name,
            index,
        } => crate::tui::log_trace!(
            "{} tool_call_start chat_id={} turn_id={} id={} provider_id={:?} name={} index={}",
            stage,
            context.chat_id,
            context.turn_id,
            id,
            provider_id,
            name,
            index
        ),
        sdk::ChatEvent::ToolCallUpdate {
            context,
            id,
            provider_id,
            name,
            index,
            arguments_delta,
            arguments,
            status,
        } => crate::tui::log_trace!(
            "{} tool_call_update chat_id={} turn_id={} id={} provider_id={:?} name={} index={} status={:?} args_delta_len={} args_present={} ",
            stage,
            context.chat_id,
            context.turn_id,
            id,
            provider_id,
            name,
            index,
            status,
            arguments_delta.as_ref().map(|value| value.len()).unwrap_or(0),
            arguments.is_some(),
        ),
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
        } => crate::tui::log_trace!(
            "{} tool_result chat_id={} turn_id={} id={} provider_id={} tool_name={} output_len={} content_kind={} is_error={} image_count={}",
            stage,
            context.chat_id,
            context.turn_id,
            id,
            provider_id,
            tool_name,
            output.len(),
            json_value_kind(content),
            is_error,
            images.len()
        ),
        sdk::ChatEvent::SystemMessage(message) => {
            crate::tui::log_trace!("{} system_message len={}", stage, message.len())
        }
        sdk::ChatEvent::ModelStreamWaiting {
            context,
            elapsed_secs,
            phase,
        } => crate::tui::log_trace!(
            "{} model_stream_waiting chat_id={} turn_id={} elapsed_secs={} phase={}",
            stage,
            context.chat_id,
            context.turn_id,
            elapsed_secs,
            phase
        ),
        sdk::ChatEvent::ModelInvocationRetrying {
            context,
            attempt,
            delay,
        } => crate::tui::log_trace!(
            "{} model_invocation_retrying chat_id={} turn_id={} attempt={} delay_ms={}",
            stage,
            context.chat_id,
            context.turn_id,
            attempt,
            delay.as_millis()
        ),
        sdk::ChatEvent::Usage {
            input,
            output,
            last_input,
            elapsed_secs,
        } => crate::tui::log_trace!(
            "{} usage input={} output={} last_input={} elapsed_secs={:.3}",
            stage,
            input,
            output,
            last_input,
            elapsed_secs
        ),
        sdk::ChatEvent::TurnStarted { messages }
        | sdk::ChatEvent::MicrocompactDone { messages, .. }
        | sdk::ChatEvent::StopHookBlocked { messages }
        | sdk::ChatEvent::PostToolExecutionSync { messages }
        | sdk::ChatEvent::CompactRollback { messages }
        | sdk::ChatEvent::CompactFinished { messages } => {
            crate::tui::log_trace!("{} messages_sync count={}", stage, messages.len())
        }
        sdk::ChatEvent::ApiError { messages, error } => {
            crate::tui::log_trace!("{} api_error count={} err={}", stage, messages.len(), error)
        }
        sdk::ChatEvent::UserMessagesAdopted { items, queued } => {
            crate::tui::log_trace!(
                "{} user_messages_adopted count={} queued={}",
                stage,
                items.len(),
                queued.len()
            )
        }
        sdk::ChatEvent::UserMessagesQueued { queued } => {
            crate::tui::log_trace!(
                "{} user_messages_queued count={}",
                stage,
                queued.len()
            )
        }
        sdk::ChatEvent::Done { context } => crate::tui::log_trace!(
            "{} done chat_id={} turn_id={}",
            stage,
            context.chat_id,
            context.turn_id
        ),
        sdk::ChatEvent::DoneWithDurationMs {
            context,
            duration_ms,
        } => crate::tui::log_trace!(
            "{} done_with_duration_ms chat_id={} turn_id={} duration_ms={}",
            stage,
            context.chat_id,
            context.turn_id,
            duration_ms
        ),
        sdk::ChatEvent::RunStarted {
            run_id,
            parent_run_id,
        } => crate::tui::log_trace!(
            "{} run_started run_id={} parent_run_id={:?}",
            stage,
            run_id,
            parent_run_id
        ),
        sdk::ChatEvent::RunStepStarted { run_id, step_id, .. } => {
            crate::tui::log_trace!("{} run_step_started run_id={} step_id={}", stage, run_id, step_id)
        }
        sdk::ChatEvent::RunStepCompleted { run_id, step_id, .. } => {
            crate::tui::log_trace!("{} run_step_completed run_id={} step_id={}", stage, run_id, step_id)
        }
        sdk::ChatEvent::RunStepCancellationRequested { run_id, step_id, .. } => crate::tui::log_trace!("{} run_step_cancellation_requested run_id={} step_id={}", stage, run_id, step_id),
        sdk::ChatEvent::RunStepFinalizationStarted { run_id, step_id, .. } => crate::tui::log_trace!("{} run_step_finalization_started run_id={} step_id={}", stage, run_id, step_id),
        sdk::ChatEvent::RunStepCancelled { run_id, step_id, confirmed, .. } => crate::tui::log_trace!("{} run_step_cancelled run_id={} step_id={} confirmed={}", stage, run_id, step_id, confirmed),
        sdk::ChatEvent::RunDrainingInput { run_id, .. } => crate::tui::log_trace!("{} run_draining_input run_id={}", stage, run_id),
        sdk::ChatEvent::RunTerminationRequested { run_id, .. } => crate::tui::log_trace!("{} run_termination_requested run_id={}", stage, run_id),
        sdk::ChatEvent::RunTerminated { run_id, .. } => crate::tui::log_trace!("{} run_terminated run_id={}", stage, run_id),
        sdk::ChatEvent::RunCompleted { run_id, .. } => crate::tui::log_trace!("{} run_completed run_id={}", stage, run_id),
        sdk::ChatEvent::RunFailed { run_id, .. } => crate::tui::log_trace!("{} run_failed run_id={}", stage, run_id),
        sdk::ChatEvent::RunStuckDetected { run_id, .. } => crate::tui::log_trace!("{} run_stuck_detected run_id={}", stage, run_id),
        sdk::ChatEvent::RunTransitioned { run_id, status, .. } => crate::tui::log_trace!("{} run_transitioned run_id={} status={}", stage, run_id, status),
        sdk::ChatEvent::RunAwaitingUser { run_id, .. } => crate::tui::log_trace!("{} run_awaiting_user run_id={}", stage, run_id),
        sdk::ChatEvent::RunResumed { run_id, .. } => crate::tui::log_trace!("{} run_resumed run_id={}", stage, run_id),
        sdk::ChatEvent::InteractionRequested { request } => crate::tui::log_trace!("{} interaction_requested request_id={} run_id={}", stage, request.id, request.run_id),
        sdk::ChatEvent::RunCancelling { run_id } => {
            crate::tui::log_trace!("{} run_cancelling run_id={}", stage, run_id)
        }
        sdk::ChatEvent::RunCancelled { run_id } => {
            crate::tui::log_trace!("{} run_cancelled run_id={}", stage, run_id)
        }
        sdk::ChatEvent::Cancelled { context } => crate::tui::log_trace!(
            "{} cancelled chat_id={} turn_id={}",
            stage,
            context.chat_id,
            context.turn_id
        ),
        sdk::ChatEvent::LiveTps(tps) => crate::tui::log_trace!("{} live_tps={:.2}", stage, tps),
        sdk::ChatEvent::TurnChanged(turn) => {
            crate::tui::log_trace!("{} turn_changed turn={}", stage, turn)
        }
        sdk::ChatEvent::CurrentTurnChanged(turn) => {
            crate::tui::log_trace!("{} current_turn_changed turn={}", stage, turn)
        }
        sdk::ChatEvent::HookEvent(event) => crate::tui::log_trace!(
            "{} hook_event name={} status={:?}",
            stage,
            event.hook_name,
            event.status
        ),
        sdk::ChatEvent::AskUserBatch { items, .. } => {
            crate::tui::log_trace!("{} ask_user_batch count={}", stage, items.len())
        }
        sdk::ChatEvent::AgentProgress {
            context,
            tool_id,
            event,
        } => crate::tui::log_trace!(
            "{} agent_progress chat_id={} turn_id={} tool_id={} seq={} kind={}",
            stage,
            context.chat_id,
            context.turn_id,
            tool_id,
            event.sequence,
            event
        ),
        sdk::ChatEvent::WorkingDirectoryChanged {
            path_base,
            workspace_root,
            workspace,
        } => crate::tui::log_trace!(
            "{} working_directory_changed path_base={} workspace_root={} context_stack_len={}",
            stage,
            path_base,
            workspace_root,
            workspace.context_stack.len()
        ),
        sdk::ChatEvent::TasksSnapshot { tasks } => {
            crate::tui::log_trace!("{} tasks_snapshot lines={}", stage, tasks.lines.len())
        }
        sdk::ChatEvent::ConfigReloaded { changed_keys } => crate::tui::log_trace!(
            "{} config_reloaded changed_keys={:?}",
            stage,
            changed_keys
        ),
        sdk::ChatEvent::GraphPhaseChanged {
            node,
            effort,
            prev,
        } => crate::tui::log_trace!(
            "{} graph_phase_changed node={} effort={} prev={}",
            stage,
            node,
            effort,
            prev
        ),
        sdk::ChatEvent::SessionReset => {
            crate::tui::log_trace!("{} session_reset", stage)
        }
        sdk::ChatEvent::UserMessagesWithdrawn { texts } => crate::tui::log_trace!(
            "{} user_messages_withdrawn count={}",
            stage,
            texts.len()
        ),
        sdk::ChatEvent::CompactProgress {
            stage: _,
            current,
            total,
        } => crate::tui::log_trace!(
            "{} compact_progress current={:?} total={:?}",
            stage,
            current,
            total,
        ),
        sdk::ChatEvent::ModelSwitched { result } => crate::tui::log_trace!(
            "{} model_switched display={} context_window={} reasoning={:?}",
            stage,
            result.display_name,
            result.context_window,
            result.reasoning_active
        ),
        sdk::ChatEvent::ThinkingChanged { enabled } => {
            crate::tui::log_trace!("{} thinking_changed enabled={}", stage, enabled)
        }
        sdk::ChatEvent::ContextEstimated {
            estimate,
            message_count,
        } => crate::tui::log_trace!(
            "{} context_estimated tokens={} system={} size={} pct={} msgs={}",
            stage,
            estimate.estimated_tokens,
            estimate.system_tokens,
            estimate.context_size,
            estimate.usage_percentage,
            message_count
        ),
        sdk::ChatEvent::CommandResultText { text, is_error } => crate::tui::log_trace!(
            "{} command_result_text len={} is_error={}",
            stage,
            text.len(),
            is_error
        ),
        sdk::ChatEvent::SessionResumed { messages, session_id, .. } => crate::tui::log_trace!(
            "{} session_resumed id={} msg_count={}",
            stage,
            session_id,
            messages.len()
        ),
        sdk::ChatEvent::Result(result) => crate::tui::log_trace!(
            "{} result text_len={} tokens_used={:?}",
            stage,
            result.text.len(),
            result.tokens_used
        ),
        // #567: 新增变体暂不记录日志。
        sdk::ChatEvent::ReflectionResult { .. }
         | sdk::ChatEvent::ModelList { .. }
         | sdk::ChatEvent::ReminderList { .. }
         | sdk::ChatEvent::SessionList { .. }
         | sdk::ChatEvent::ProjectInfo { .. }
         | sdk::ChatEvent::CostUpdate { .. }
         | sdk::ChatEvent::SessionResumeFailed { .. } => {}
    }
}

pub(super) fn log_ui_tool_event(event: &UiEvent, stage: &'static str) {
    match event {
        UiEvent::ToolCallStart {
            context,
            id,
            provider_id,
            name,
            index,
        } => crate::tui::log_trace!(
            "{} tool_call_start chat_id={} turn_id={} id={} provider_id={:?} name={} index={}",
            stage,
            context.chat_id,
            context.turn_id,
            id,
            provider_id,
            name,
            index
        ),
        UiEvent::ToolCallUpdate {
            context,
            id,
            provider_id,
            name,
            index,
            arguments_delta,
            arguments,
            status,
        } => crate::tui::log_trace!(
            "{} tool_call_update chat_id={} turn_id={} id={} provider_id={:?} name={} index={} status={:?} args_delta_len={} args_present={} ",
            stage,
            context.chat_id,
            context.turn_id,
            id,
            provider_id,
            name,
            index,
            status,
            arguments_delta.as_ref().map(|value| value.len()).unwrap_or(0),
            arguments.is_some(),
        ),
        UiEvent::ToolResult {
            context,
            id,
            provider_id,
            tool_name,
            output,
            content,
            is_error,
            images,
        } => crate::tui::log_trace!(
            "{} tool_result chat_id={} turn_id={} id={} provider_id={} tool_name={} output_len={} content_kind={} is_error={} image_count={}",
            stage,
            context.chat_id,
            context.turn_id,
            id,
            provider_id,
            tool_name,
            output.len(),
            json_value_kind(content),
            is_error,
            images.len()
        ),
        _ => {}
    }
}

fn json_value_kind(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "bool",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}
