use crate::tui::app::event::UiEvent;

pub(crate) fn log_sdk_event(event: &sdk::ChatEvent, stage: &'static str) {
    match event {
        sdk::ChatEvent::Token { context, text } => crate::tui::log_trace!(
            "{} token chat_id={} run_id={} text_len={}",
            stage,
            context.chat_id,
            context.run_id,
            text.len()
        ),
        sdk::ChatEvent::Thinking { context, text } => crate::tui::log_trace!(
            "{} thinking chat_id={} run_id={} text_len={}",
            stage,
            context.chat_id,
            context.run_id,
            text.len()
        ),
        sdk::ChatEvent::BlockComplete { context, text } => crate::tui::log_trace!(
            "{} block_complete chat_id={} run_id={} text_len={}",
            stage,
            context.chat_id,
            context.run_id,
            text.len()
        ),
        sdk::ChatEvent::ToolCallStart {
            context,
            id,
            provider_id,
            name,
            index,
        } => crate::tui::log_trace!(
            "{} tool_call_start chat_id={} run_id={} id={} provider_id={:?} name={} index={}",
            stage,
            context.chat_id,
            context.run_id,
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
            "{} tool_call_update chat_id={} run_id={} id={} provider_id={:?} name={} index={} status={:?} args_delta_len={} args_present={} ",
            stage,
            context.chat_id,
            context.run_id,
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
            "{} tool_result chat_id={} run_id={} id={} provider_id={} tool_name={} output_len={} content_kind={} is_error={} image_count={}",
            stage,
            context.chat_id,
            context.run_id,
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
        sdk::ChatEvent::ModelInvocationRetrying {
            context,
            attempt,
            delay,
        } => crate::tui::log_trace!(
            "{} model_invocation_retrying chat_id={} run_id={} attempt={} delay_ms={}",
            stage,
            context.chat_id,
            context.run_id,
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
        | sdk::ChatEvent::CompactRollback { messages }
        | sdk::ChatEvent::CompactFinished { messages, .. } => {
            crate::tui::log_trace!("{} messages_sync count={}", stage, messages.len())
        }
        sdk::ChatEvent::SessionMessageStateChanged {
            message_count,
            revision,
        } => crate::tui::log_trace!(
            "{} message_state_changed count={} revision={}",
            stage,
            message_count,
            revision
        ),
        sdk::ChatEvent::HookNotice { notice } => crate::tui::log_trace!(
            "{} hook_notice point={} command={} exit_code={:?} has_output_file={}",
            stage,
            notice.point,
            notice.command,
            notice.exit_code,
            notice.output_file.is_some()
        ),
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
            "{} done chat_id={} run_id={}",
            stage,
            context.chat_id,
            context.run_id
        ),
        sdk::ChatEvent::DoneWithDurationMs {
            context,
            duration_ms,
        } => crate::tui::log_trace!(
            "{} done_with_duration_ms chat_id={} run_id={} duration_ms={}",
            stage,
            context.chat_id,
            context.run_id,
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
        sdk::ChatEvent::RunTransitioned { run_id, status, .. } => crate::tui::log_trace!("{} run_transitioned run_id={} status={:?}", stage, run_id, status),
        sdk::ChatEvent::RunAwaitingUser { run_id, .. } => crate::tui::log_trace!("{} run_awaiting_user run_id={}", stage, run_id),
        sdk::ChatEvent::RunResumed { run_id, .. } => crate::tui::log_trace!("{} run_resumed run_id={}", stage, run_id),
        sdk::ChatEvent::InteractionRequested { request } => crate::tui::log_trace!("{} interaction_requested request_id={} run_id={}", stage, request.id, request.run_id),
        sdk::ChatEvent::Cancelled { context, .. } => crate::tui::log_trace!(            "{} cancelled chat_id={} run_id={}",
            stage,
            context.chat_id,
            context.run_id
        ),
        sdk::ChatEvent::LiveTps(tps) => crate::tui::log_trace!("{} live_tps={:.2}", stage, tps),
        sdk::ChatEvent::RunChanged(run_step) => {
            crate::tui::log_trace!("{} run_changed run_step={}", stage, run_step)
        }
        sdk::ChatEvent::CurrentRunChanged(run_step) => {
            crate::tui::log_trace!("{} current_run_changed run_step={}", stage, run_step)
        }
        sdk::ChatEvent::AskUserBatch { items, .. } => {
            crate::tui::log_trace!("{} ask_user_batch count={}", stage, items.len())
        }
        sdk::ChatEvent::AgentProgress {
            source_context,
            attachment_context,
            tool_id,
            event,
        } => crate::tui::log_trace!(
            "{} agent_progress source_chat_id={} source_run_id={} attachment_chat_id={} attachment_run_id={} tool_id={} seq={} kind={}",
            stage,
            source_context.chat_id,
            source_context.run_id,
            attachment_context.chat_id,
            attachment_context.run_id,
            tool_id,
            event.sequence,
            event
        ),
        sdk::ChatEvent::ChildRunActivity { event } => crate::tui::log_trace!(
            "{} child_run_activity agent_id={} run_id={} parent_run_id={} tool_id={} seq={}",
            stage,
            event.identity.agent_id,
            event.identity.run_id,
            event.identity.parent_run_id,
            event.identity.spawned_by_tool_call_id,
            event.sequence
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
        sdk::ChatEvent::TaskStateChanged { state } => crate::tui::log_trace!(
            "{} task_state_changed session_id={} revision={} items={}",
            stage,
            state.session_id,
            state.revision,
            state.items.len()
        ),
        sdk::ChatEvent::ConfigChanged { event } => crate::tui::log_trace!(
            "{} config_changed fields={:?}",
            stage,
            event.changed_fields
        ),
        sdk::ChatEvent::ConfigReloaded { event } => crate::tui::log_trace!(
            "{} config_reloaded changed_keys={:?} scopes={:?}",
            stage,
            event.changed_keys,
            event.scopes
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
        sdk::ChatEvent::SessionResumed { steps, session_id, .. } => crate::tui::log_debug!(
            "resume_lifecycle boundary=sdk_stream stage=session_resumed_received session_id={} steps={} messages={}",
            session_id,
            steps.len(),
            steps.iter().map(|step| step.messages.len()).sum::<usize>()
        ),        sdk::ChatEvent::Result(result) => crate::tui::log_trace!(
            "{} result text_len={} tokens_used={:?}",
            stage,
            result.text.len(),
            result.tokens_used
        ),
        sdk::ChatEvent::ReflectionHistory { records } => crate::tui::log_trace!(
            "{} reflection_history count={}",
            stage,
            records.len()
        ),
        sdk::ChatEvent::ActivityChanged { kind, activity } => crate::tui::log_debug!(
            "{} activity_changed run_id={} activity_id={} source={:?} kind={:?} state={:?} change={:?} revision={} total_elapsed_ms={} active_elapsed_ms={} state_elapsed_ms={}",
            stage,
            activity.run_id,
            activity.id,
            activity.source,
            activity.kind,
            activity.state,
            kind,
            activity.revision,
            activity.timing.total_elapsed_ms,
            activity.timing.active_elapsed_ms,
            activity.timing.state_elapsed_ms,
        ),
        sdk::ChatEvent::ActivitySnapshot(snapshot) => crate::tui::log_debug!(
            "{} activity_snapshot run_id={} revision={} activity_count={}",
            stage,
            snapshot.run_id,
            snapshot.revision,
            snapshot.activities.len(),
        ),
        // These metadata/list events are intentionally omitted from trace logging.
        sdk::ChatEvent::SkillsUpdated { .. }
        | sdk::ChatEvent::ModelList { .. }
         | sdk::ChatEvent::ReminderList { .. }
         | sdk::ChatEvent::SessionList { .. }
         | sdk::ChatEvent::ProjectInfo { .. }
         | sdk::ChatEvent::CostUpdate { .. }
         | sdk::ChatEvent::SessionResumeFailed { .. } => {}    }
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
            "{} tool_call_start chat_id={} run_id={} id={} provider_id={:?} name={} index={}",
            stage,
            context.chat_id,
            context.run_id,
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
            "{} tool_call_update chat_id={} run_id={} id={} provider_id={:?} name={} index={} status={:?} args_delta_len={} args_present={} ",
            stage,
            context.chat_id,
            context.run_id,
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
            "{} tool_result chat_id={} run_id={} id={} provider_id={} tool_name={} output_len={} content_kind={} is_error={} image_count={}",
            stage,
            context.chat_id,
            context.run_id,
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

pub(crate) fn log_tui_runtime_delivery(
    event: &crate::tui::adapter::tui_runtime_event::TuiRuntimeEvent,
    outcome: &'static str,
) {
    use crate::tui::adapter::tui_runtime_event::TuiRuntimeEvent;

    match event {
        TuiRuntimeEvent::Text { context, text } => crate::tui::log_trace!(
            "event_delivery boundary=sdk_to_tui kind=Text chat_id={} run_id={} size={} outcome={}",
            context.chat_id,
            context.run_id,
            text.len(),
            outcome
        ),
        TuiRuntimeEvent::BlockComplete { context, text } => crate::tui::log_debug!(
            "event_delivery boundary=sdk_to_tui kind=BlockComplete chat_id={} run_id={} size={} outcome={}",
            context.chat_id,
            context.run_id,
            text.len(),
            outcome
        ),
        TuiRuntimeEvent::UserMessagesAdopted { items, queued } => crate::tui::log_debug!(
            "event_delivery boundary=sdk_to_tui kind=UserMessagesAdopted items={} queued={} outcome={}",
            items.len(),
            queued.len(),
            outcome
        ),
        TuiRuntimeEvent::Done { context, .. } => crate::tui::log_debug!(
            "event_delivery boundary=sdk_to_tui kind=Done chat_id={} run_id={} size=0 outcome={}",
            context.chat_id,
            context.run_id,
            outcome
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
