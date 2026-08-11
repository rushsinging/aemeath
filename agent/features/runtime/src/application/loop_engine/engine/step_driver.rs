use super::*;

pub(super) async fn execute_step(
    run: &mut Run,
    execution: &mut RunExecutionState,
    cancel: &CancellationToken,
    port: &mut RunLoop<'_>,
    guard: &mut StuckGuard,
    inputs: &[LoopInput],
    terminal_text: &mut Option<String>,
) -> Result<(), LoopEngineError> {
    let step_cancel = cancel.child_token();
    let step_id = sdk::RunStepId::new_v7();
    log::debug!(
        target: crate::LOG_TARGET,
        "step cancellation scope created: run_id={} step_id={} root_cancelled={} step_cancelled={}",
        run.id(),
        step_id,
        cancel.is_cancelled(),
        step_cancel.is_cancelled()
    );
    port.register_step_scope(run.id(), step_id.clone(), step_cancel.clone());
    let result = execute_step_with_scope(
        run,
        execution,
        cancel,
        port,
        guard,
        inputs,
        terminal_text,
        step_id.clone(),
        step_cancel,
    )
    .await;
    port.clear_step_scope(run.id(), &step_id);
    result
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn execute_step_with_scope(
    run: &mut Run,
    execution: &mut RunExecutionState,
    cancel: &CancellationToken,
    port: &mut RunLoop<'_>,
    guard: &mut StuckGuard,
    inputs: &[LoopInput],
    terminal_text: &mut Option<String>,
    step_id: sdk::RunStepId,
    step_cancel: CancellationToken,
) -> Result<(), LoopEngineError> {
    let step_id = match run_step_input_phase(
        run,
        execution,
        step_id,
        inputs,
        port.persistence_mut(),
    )
    .await?
    {
        StepInputOutcome::Accepted(step_id) => step_id,
        StepInputOutcome::Rejected(error) => {
            fail_run(run, execution, port, error.to_string()).await?;
            return Ok(());
        }
    };
    emit_events(run, execution, port).await?;
    // -- compaction check --
    let needs_compaction =
        match await_interruptible(run, &step_cancel, port.needs_compaction(execution)).await {
            Interrupt::Completed(result) => result?,
            Interrupt::Cancelled => {
                handle_step_control(run, execution, port).await?;
                return Ok(());
            }
            Interrupt::TimedOut => {
                timeout_run(run, execution, port).await?;
                return Ok(());
            }
        };
    if needs_compaction {
        transition_and_emit(run, execution, port, RunTransition::BeginCompaction).await?;
        let compact_activity_id = port.start_compaction_activity(step_id.clone())?;
        // #1500：进度由 Context 的 compact 管线（Preparing/Summarizing
        // chunk 计数/Finalizing）经 progress 回调驱动，不再硬编码 Summarizing。
        let compact_progress = port.compact_progress_view(compact_activity_id.clone());
        match run_context_compaction_phase(
            run,
            execution,
            &step_cancel,
            port.compaction_mut(),
            compact_progress,
        )
        .await?
        {
            ContextCompactionOutcome::Ready => {
                port.update_compaction_activity(
                    compact_activity_id.clone(),
                    sdk::CompactStageView::Finalizing,
                )?;
                port.finish_activity(compact_activity_id, ActivityTerminal::Succeeded)?;
            }
            ContextCompactionOutcome::Cancelled => {
                port.finish_activity(compact_activity_id, ActivityTerminal::Cancelled)?;
                return handle_step_control(run, execution, port).await;
            }
            ContextCompactionOutcome::TimedOut => {
                port.finish_activity(compact_activity_id, ActivityTerminal::Terminated)?;
                timeout_run(run, execution, port).await?;
                return Ok(());
            }
        }
        transition_and_emit(run, execution, port, RunTransition::CompactionCompleted).await?;
    }

    if handle_interrupt(run, execution, cancel, port).await? {
        return Ok(());
    }
    transition_and_emit(run, execution, port, RunTransition::ContextPrepared).await?;
    let model_parent_activity_id = port.activity_parent_id()?;
    let model_name = port.model_name()?.to_string();
    let mut model_attempt = 1_u32;
    let mut model_invocation_id = sdk::ModelInvocationId::new_v7();
    let mut model_activity_id = port.start_model_activity(
        step_id.clone(),
        model_parent_activity_id,
        model_invocation_id.clone(),
        model_name.clone(),
        model_attempt,
    )?;
    let mut compacted_after_context_too_long = false;
    let (model_step, token_usage) = loop {
        match run_model_invocation_phase(
            run,
            execution,
            &step_id,
            &model_invocation_id,
            &step_cancel,
            port.model_mut(),
        )
        .await
        {
            ModelInvocationOutcome::Invoked(step, usage) => {
                port.update_model_activity(
                    model_activity_id.clone(),
                    model_name.clone(),
                    model_attempt,
                    sdk::ModelStreamStateView::Streaming,
                )?;
                port.finish_activity(model_activity_id, ActivityTerminal::Succeeded)?;
                break (step, usage);
            }
            ModelInvocationOutcome::Cancelled => {
                port.finish_activity(model_activity_id, ActivityTerminal::Cancelled)?;
                handle_step_control(run, execution, port).await?;
                return Ok(());
            }
            ModelInvocationOutcome::NeedsCompaction(error) => {
                port.finish_activity(model_activity_id, ActivityTerminal::Failed)?;
                if compacted_after_context_too_long {
                    fail_run(
                        run,
                        execution,
                        port,
                        format!("compact 后 Provider 仍报告 context 超限：{error}"),
                    )
                    .await?;
                    return Ok(());
                }
                transition_and_emit(run, execution, port, RunTransition::ModelContextExceeded)
                    .await?;
                let compact_activity_id = port.start_compaction_activity(step_id.clone())?;
                // #1500：进度由 Context compact 管线经 progress 回调驱动。
                let compact_progress = port.compact_progress_view(compact_activity_id.clone());
                match run_context_compaction_phase(
                    run,
                    execution,
                    &step_cancel,
                    port.compaction_mut(),
                    compact_progress,
                )
                .await?
                {
                    ContextCompactionOutcome::Ready => {
                        port.update_compaction_activity(
                            compact_activity_id.clone(),
                            sdk::CompactStageView::Finalizing,
                        )?;
                        port.finish_activity(compact_activity_id, ActivityTerminal::Succeeded)?;
                    }
                    ContextCompactionOutcome::Cancelled => {
                        port.finish_activity(compact_activity_id, ActivityTerminal::Cancelled)?;
                        handle_step_control(run, execution, port).await?;
                        return Ok(());
                    }
                    ContextCompactionOutcome::TimedOut => {
                        port.finish_activity(compact_activity_id, ActivityTerminal::Terminated)?;
                        timeout_run(run, execution, port).await?;
                        return Ok(());
                    }
                }
                transition_and_emit(run, execution, port, RunTransition::CompactionCompleted)
                    .await?;
                transition_and_emit(run, execution, port, RunTransition::ContextPrepared).await?;
                model_attempt += 1;
                model_invocation_id = sdk::ModelInvocationId::new_v7();
                model_activity_id = port.start_model_activity(
                    step_id.clone(),
                    port.activity_parent_id()?,
                    model_invocation_id.clone(),
                    model_name.clone(),
                    model_attempt,
                )?;
                port.update_model_activity(
                    model_activity_id.clone(),
                    model_name.clone(),
                    model_attempt,
                    sdk::ModelStreamStateView::Retrying,
                )?;
                compacted_after_context_too_long = true;
            }
            ModelInvocationOutcome::Failed(error) => {
                port.finish_activity(model_activity_id, ActivityTerminal::Failed)?;
                fail_run(run, execution, port, error.to_string()).await?;
                return Ok(());
            }
            ModelInvocationOutcome::TimedOut => {
                port.finish_activity(model_activity_id, ActivityTerminal::Terminated)?;
                timeout_run(run, execution, port).await?;
                return Ok(());
            }
        }
    };

    // Per-step token usage + context window 诊断日志
    {
        let ctx_win = token_usage.context_window;
        let total = token_usage.total_tokens;
        let pct = total
            .checked_mul(100)
            .and_then(|v| v.checked_div(ctx_win))
            .map(|v| v as u32)
            .unwrap_or(0);
        log::info!(
            target: crate::LOG_TARGET,
            "token usage: input={} (cached {}) | output={} (cache_write {}) | reasoning={} | total={} | context_window={} | {pct}% \
             | stop_reason={} | est: system={} tools={} messages={} total_est={}",
            token_usage.input_tokens,
            token_usage.cached_tokens,
            token_usage.output_tokens,
            token_usage.cache_creation_tokens,
            token_usage.reasoning_tokens,
            total,
            ctx_win,
            token_usage.stop_reason,
            token_usage.est_system_tokens,
            token_usage.est_tool_tokens,
            token_usage.est_message_tokens,
            token_usage.est_total(),
        );
    }
    if handle_interrupt(run, execution, cancel, port).await? {
        return Ok(());
    }
    run.record_model_invocation(&step_id, model_invocation(&model_step))?;
    transition_and_emit(run, execution, port, RunTransition::ModelInvoked).await?;
    log::debug!(
        target: crate::LOG_TARGET,
        "[run_loop] model_step={} run_id={}",
        model_step_label(&model_step),
        short(run.id()),
    );

    // #1272: track the last assistant text for terminal claim
    let assistant_text = model_step_text(&model_step);
    *terminal_text = Some(assistant_text);

    match model_step {
        ModelStep::Complete { text } => {
            // Text-only completion is handled by the static reasoning level.

            // #1248 Task 5: Plan approval before proceeding when in plan mode.
            if port.needs_plan_approval() && !text.trim().is_empty() {
                handle_plan_approval(run, execution, port, &step_id, &text).await?;
                return Ok(());
            }

            // #1248 Task 6: Evaluate Stop hook BEFORE text stall check.
            // A blocking stop hook with repeated output should continue
            // (feedback may change the model's behavior); text stall
            // detection runs only when the stop hook allows proceeding.
            let stop_outcome = match port
                .coordinate_stop_hook(execution, &step_id, run.steps().len(), &step_cancel)
                .await
            {
                Ok(outcome) => outcome,
                Err(error) => return Err(error),
            };
            match stop_outcome.decision {
                StopHookDecision::Proceed => {
                    // Normal completion — fall through to text stall check.
                }
                StopHookDecision::Cancelled => {
                    return handle_step_control(run, execution, port).await;
                }
                StopHookDecision::Block(ref block) => {
                    let block_result = run.record_stop_hook_block();
                    log::info!(
                        target: crate::LOG_TARGET,
                        "[stop_hook] blocked: reason={:?} count={}",
                        block.reason,
                        run.stop_hook_block_count(),
                    );
                    match block_result {
                        StopHookBlockResult::Blocked { .. } => {
                            // #1272: Block → ContinueAfterResponse for another attempt.
                            transition_and_emit(
                                run,
                                execution,
                                port,
                                RunTransition::ContinueAfterResponse,
                            )
                            .await?;
                            run.complete_step(&step_id)?;
                            run_step_finalization_phase(
                                execution,
                                port.persistence_mut(),
                                &step_id,
                                crate::ports::FinalizeCause::Completed,
                            )
                            .await?;
                            return Ok(());
                        }
                        StopHookBlockResult::RetryExhausted { count } => {
                            // 16th block → Run Failed.
                            fail_run(
                                run,
                                execution,
                                port,
                                format!(
                                    "stop hook blocked completion {count} times (retry exhausted)"
                                ),
                            )
                            .await?;
                            return Ok(());
                        }
                    }
                }
            }

            // Text stall detection only after stop hook allows proceeding.
            match guard.inspect_text(terminal_text.as_deref().unwrap_or("")) {
                decision @ StuckDecision::SoftBlock { .. } => {
                    record_stuck(run, execution, port, &decision).await?;
                    transition_and_emit(run, execution, port, RunTransition::ContinueAfterResponse)
                        .await?;
                    run.complete_step(&step_id)?;
                    run_step_finalization_phase(
                        execution,
                        port.persistence_mut(),
                        &step_id,
                        crate::ports::FinalizeCause::Completed,
                    )
                    .await?;
                    return Ok(());
                }
                decision @ StuckDecision::HardPause { .. } => {
                    let reason = match &decision {
                        StuckDecision::HardPause { reason } => reason.clone(),
                        _ => unreachable!(),
                    };
                    record_stuck(run, execution, port, &decision).await?;
                    handle_hard_pause(run, execution, port, &step_id, reason).await?;
                    return Ok(());
                }
                StuckDecision::Allow => {}
            }

            // #1272: Complete goes to DrainingInput (not Finishing→Finish)
            transition_and_emit(run, execution, port, RunTransition::ContinueAfterResponse).await?;
            run.complete_step(&step_id)?;
            run_step_finalization_phase(
                execution,
                port.persistence_mut(),
                &step_id,
                crate::ports::FinalizeCause::Completed,
            )
            .await?;
            // Loop back to drain — adapter returns EmptyAndSealed for Complete
        }
        #[cfg(test)]
        ModelStep::Continue { text: _ } => {
            let decision = guard.inspect_text(terminal_text.as_deref().unwrap_or(""));
            match decision {
                StuckDecision::SoftBlock { .. } => {
                    record_stuck(run, execution, port, &decision).await?
                }
                StuckDecision::HardPause { ref reason } => {
                    let reason = reason.clone();
                    record_stuck(run, execution, port, &decision).await?;
                    handle_hard_pause(run, execution, port, &step_id, reason).await?;
                    return Ok(());
                }
                StuckDecision::Allow => {}
            }
            // #1272: Continue goes to DrainingInput
            transition_and_emit(run, execution, port, RunTransition::ContinueAfterResponse).await?;
            run.complete_step(&step_id)?;
            run_step_finalization_phase(
                execution,
                port.persistence_mut(),
                &step_id,
                crate::ports::FinalizeCause::Completed,
            )
            .await?;
        }
        ModelStep::Tools { text: _, calls } => {
            if let decision @ StuckDecision::SoftBlock { .. } =
                guard.inspect_text(terminal_text.as_deref().unwrap_or(""))
            {
                record_stuck(run, execution, port, &decision).await?;
            }
            transition_and_emit(run, execution, port, RunTransition::ResponseWithTools).await?;
            let mut guarded_calls = Vec::with_capacity(calls.len());
            for call in calls {
                run.add_tool_call(&step_id, call.clone())?;
                match guard.inspect_tool(&call) {
                    StuckDecision::SoftBlock { reason } => {
                        record_stuck(
                            run,
                            execution,
                            port,
                            &StuckDecision::SoftBlock {
                                reason: reason.clone(),
                            },
                        )
                        .await?;
                        guarded_calls.push((call, ToolGuardDecision::SoftBlock { reason }));
                    }
                    StuckDecision::HardPause { reason } => {
                        record_stuck(
                            run,
                            execution,
                            port,
                            &StuckDecision::HardPause {
                                reason: reason.clone(),
                            },
                        )
                        .await?;
                        handle_hard_pause(run, execution, port, &step_id, reason).await?;
                        return Ok(());
                    }
                    StuckDecision::Allow => {
                        guarded_calls.push((call, ToolGuardDecision::Allow));
                    }
                }
            }
            let parallel_count = u16::try_from(guarded_calls.len()).unwrap_or(u16::MAX);
            let tool_parent_activity_id = port.activity_parent_id()?;
            let tool_activity_ids = guarded_calls
                .iter()
                .map(|(call, _)| {
                    port.start_tool_activity(
                        step_id.clone(),
                        tool_parent_activity_id.clone(),
                        call,
                        parallel_count,
                    )
                    .map(|activity_id| (call.id.clone(), activity_id))
                })
                .collect::<Result<std::collections::HashMap<_, _>, ActivityError>>()?;
            for (call, _) in &guarded_calls {
                run.advance_tool_call(&step_id, &call.id, ToolCallStatus::Ready)?;
            }
            transition_and_emit(run, execution, port, RunTransition::ToolsApproved).await?;
            for (call, decision) in &guarded_calls {
                if matches!(decision, ToolGuardDecision::Allow) {
                    run.advance_tool_call(&step_id, &call.id, ToolCallStatus::Running)?;
                }
            }
            log::debug!(
                target: crate::LOG_TARGET,
                "[run_loop] tool round path=standard count={} run_id={}",
                guarded_calls.len(),
                short(run.id()),
            );
            // #1494：边流边执行——流中 ToolCallCompleted 已旁路执行（结果缓冲非空）时，
            // 跳过 execute_tools 重复执行，直接汇总缓冲结果；缓冲为空（非流式 / 未装配）
            // 时走现状工具轮次。
            let streaming_rounds = port.model_mut().take_streaming_tool_results().await;
            let tool_outcome = if streaming_rounds.is_empty() {
                match run_tool_round_phase(
                    run,
                    execution,
                    run.id(),
                    &step_id,
                    &guarded_calls,
                    &step_cancel,
                    port.tools_mut(),
                )
                .await
                {
                    ToolRoundPhaseOutcome::Completed(outcome) => outcome,
                    ToolRoundPhaseOutcome::Cancelled => {
                        for activity_id in tool_activity_ids.values() {
                            port.finish_activity(activity_id.clone(), ActivityTerminal::Cancelled)?;
                        }
                        handle_step_control(run, execution, port).await?;
                        return Ok(());
                    }
                    ToolRoundPhaseOutcome::Failed(error) => {
                        for activity_id in tool_activity_ids.values() {
                            port.finish_activity(activity_id.clone(), ActivityTerminal::Failed)?;
                        }
                        fail_run(run, execution, port, error.to_string()).await?;
                        return Ok(());
                    }
                    ToolRoundPhaseOutcome::TimedOut => {
                        for activity_id in tool_activity_ids.values() {
                            port.finish_activity(
                                activity_id.clone(),
                                ActivityTerminal::Terminated,
                            )?;
                        }
                        timeout_run(run, execution, port).await?;
                        return Ok(());
                    }
                }
            } else {
                log::debug!(
                    target: crate::LOG_TARGET,
                    "[run_loop] tool round path=streaming rounds={} run_id={} (bypassed execute_tools)",
                    streaming_rounds.len(),
                    short(run.id()),
                );
                match port
                    .tools_mut()
                    .finalize_streaming_tool_results(
                        execution,
                        &step_id,
                        streaming_rounds,
                        &step_cancel,
                    )
                    .await
                {
                    Ok(outcome) => outcome,
                    Err(LoopEngineError::Cancelled) => {
                        for activity_id in tool_activity_ids.values() {
                            port.finish_activity(activity_id.clone(), ActivityTerminal::Cancelled)?;
                        }
                        handle_step_control(run, execution, port).await?;
                        return Ok(());
                    }
                    Err(error) => {
                        for activity_id in tool_activity_ids.values() {
                            port.finish_activity(activity_id.clone(), ActivityTerminal::Failed)?;
                        }
                        fail_run(run, execution, port, error.to_string()).await?;
                        return Ok(());
                    }
                }
            };
            if handle_interrupt(run, execution, cancel, port).await? {
                return Ok(());
            }
            if matches!(
                tool_outcome.continuation,
                crate::application::tool::coordination::ToolRoundContinuation::ToolResults
            ) {
                port.schedule_internal_continuation(InternalContinuationKind::ToolResults);
            }
            let tool_step = tool_outcome.step;
            let (fuse_bypassed, completed_non_interaction): (
                &[sdk::ToolCallId],
                &[(sdk::ToolCallId, ToolCallStatus)],
            ) = match &tool_step {
                ToolStep::ContinueWithFuseBypass(ids) => (ids.as_slice(), &[]),
                ToolStep::InteractionSuspended {
                    fuse_bypassed,
                    completed_results,
                    ..
                }
                | ToolStep::AwaitingToolApproval {
                    fuse_bypassed,
                    completed_results,
                    ..
                } => (fuse_bypassed.as_slice(), completed_results.as_slice()),
                ToolStep::Continue => (&[], &[]),
                #[cfg(test)]
                ToolStep::AwaitUser => (&[], &[]),
            };
            // #1248: For interaction steps, advance completed (non-interaction) results
            // before the interaction coordinator takes over.  Interaction calls are NOT
            // advanced here — the interaction path manages them via `advance_tool_call`.
            if !completed_non_interaction.is_empty() {
                for (call_id, status) in completed_non_interaction {
                    run.advance_tool_call(&step_id, call_id, *status)?;
                    if let Some(activity_id) = tool_activity_ids.get(call_id) {
                        let terminal = match status {
                            ToolCallStatus::Success => ActivityTerminal::Succeeded,
                            ToolCallStatus::Error => ActivityTerminal::Failed,
                            ToolCallStatus::Cancelled => ActivityTerminal::Cancelled,
                            ToolCallStatus::Pending
                            | ToolCallStatus::Ready
                            | ToolCallStatus::Running => continue,
                        };
                        port.finish_activity(activity_id.clone(), terminal)?;
                    }
                }
            }
            // Only advance tool calls for non-interaction steps.
            // Interaction paths manage tool call status themselves.
            let is_interaction = matches!(
                &tool_step,
                ToolStep::InteractionSuspended { .. } | ToolStep::AwaitingToolApproval { .. }
            );
            if !is_interaction {
                for (call, decision) in &guarded_calls {
                    let bypassed = fuse_bypassed.contains(&call.id);
                    let status = if matches!(decision, ToolGuardDecision::Allow) || bypassed {
                        ToolCallStatus::Success
                    } else {
                        ToolCallStatus::Cancelled
                    };
                    run.advance_tool_call(&step_id, &call.id, status)?;
                    if let Some(activity_id) = tool_activity_ids.get(&call.id) {
                        let terminal = match status {
                            ToolCallStatus::Success => ActivityTerminal::Succeeded,
                            ToolCallStatus::Error => ActivityTerminal::Failed,
                            ToolCallStatus::Cancelled => ActivityTerminal::Cancelled,
                            ToolCallStatus::Pending
                            | ToolCallStatus::Ready
                            | ToolCallStatus::Running => continue,
                        };
                        port.finish_activity(activity_id.clone(), terminal)?;
                    }
                }
            }
            match tool_step {
                ToolStep::Continue | ToolStep::ContinueWithFuseBypass(_) => {
                    run.complete_step(&step_id)?;
                    run_step_finalization_phase(
                        execution,
                        port.persistence_mut(),
                        &step_id,
                        crate::ports::FinalizeCause::Completed,
                    )
                    .await?;
                    // #1272: ToolsCompleted → DrainingInput (not PreparingContext)
                    transition_and_emit(run, execution, port, RunTransition::ToolsCompleted)
                        .await?;
                }
                #[cfg(test)]
                ToolStep::AwaitUser => {
                    run.complete_step(&step_id)?;
                    // AwaitUser 前必须先 finalize step outcome（模型回复 +
                    // 工具结果），否则 Terminate 时 active_step 为 None，
                    // 上一 step 的 outcome 永久丢失。
                    run_step_finalization_phase(
                        execution,
                        port.persistence_mut(),
                        &step_id,
                        crate::ports::FinalizeCause::Completed,
                    )
                    .await?;
                    transition_and_emit(run, execution, port, RunTransition::AwaitUser).await?;
                    // Return to caller; the caller will call run_loop again
                    // with drain_input picking up the user response.
                    return Ok(());
                }
                ToolStep::InteractionSuspended { suspended, .. } => {
                    // #1248 Task 5: Resolve suspensions through coordinator
                    handle_suspensions(run, execution, port, suspended).await?;
                }
                ToolStep::AwaitingToolApproval {
                    calls_needing_approval,
                    ..
                } => {
                    // #1248 Task 5: Resolve tool approvals through coordinator
                    handle_tool_approvals(run, execution, port, calls_needing_approval).await?;
                }
            }
        }
    }
    Ok(())
}
