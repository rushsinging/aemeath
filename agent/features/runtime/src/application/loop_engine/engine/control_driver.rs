use super::*;

pub(super) async fn record_stuck(
    run: &mut Run,
    execution: &mut RunExecutionState,
    port: &mut RunLoop<'_>,
    decision: &StuckDecision,
) -> Result<(), LoopEngineError> {
    let reason = match decision {
        StuckDecision::SoftBlock { reason } | StuckDecision::HardPause { reason } => reason.clone(),
        StuckDecision::Allow => return Ok(()),
    };
    run.mark_stuck(reason)?;
    emit_events(run, execution, port).await?;
    port.on_stuck(execution, decision).await
}

pub(super) enum ControlDirective {
    Continue,
    Terminal,
}

pub(super) async fn handle_pending_control(
    run: &mut Run,
    execution: &mut RunExecutionState,
    port: &mut RunLoop<'_>,
) -> Result<Option<ControlDirective>, LoopEngineError> {
    let Some(control) = port.take_control(run.id()) else {
        log::trace!(
            target: crate::LOG_TARGET,
            "run control boundary: run_id={} control=none active_step={:?}",
            run.id(),
            run.active_step_id()
        );
        return Ok(None);
    };
    log::debug!(
        target: crate::LOG_TARGET,
        "run control boundary: run_id={} control_received={control:?} active_step={:?}",
        run.id(),
        run.active_step_id()
    );
    let active_step = run.active_step_id();
    match control {
        crate::domain::agent_run::RunControl::CancelStep { step_id, .. } => {
            if active_step.as_ref() != Some(&step_id) {
                return Err(LoopEngineError::Adapter(
                    "CancelRunStep 与当前 Step identity 不匹配".to_string(),
                ));
            }
            finish_cancelled_step(run, execution, port, &step_id).await?;
            Ok(Some(ControlDirective::Continue))
        }
        crate::domain::agent_run::RunControl::Terminate { reason, deadline } => {
            match run.request_termination(reason, deadline) {
                crate::domain::agent_run::RunTerminationRequest::Accepted => {}
                crate::domain::agent_run::RunTerminationRequest::AlreadyTerminating
                | crate::domain::agent_run::RunTerminationRequest::AlreadyTerminal => {
                    return Ok(Some(ControlDirective::Terminal));
                }
            }
            emit_events(run, execution, port).await?;
            if let Some(step_id) = active_step {
                run_step_finalization_phase(
                    execution,
                    port.persistence_mut(),
                    &step_id,
                    crate::ports::FinalizeCause::UserCancelledStep,
                )
                .await?;
            }
            run.finish_termination()?;
            emit_events(run, execution, port).await?;
            Ok(Some(ControlDirective::Terminal))
        }
    }
}

pub(super) async fn finish_cancelled_step(
    run: &mut Run,
    execution: &mut RunExecutionState,
    port: &mut RunLoop<'_>,
    step_id: &sdk::RunStepId,
) -> Result<(), LoopEngineError> {
    log::debug!(
        target: crate::LOG_TARGET,
        "step cancellation finalization started: run_id={} step_id={}",
        run.id(),
        step_id
    );
    match run.request_step_cancellation(step_id) {
        crate::domain::agent_run::RunStepCancellationRequest::Accepted => {}
        crate::domain::agent_run::RunStepCancellationRequest::AlreadyCancelling => return Ok(()),
        outcome => {
            return Err(LoopEngineError::Adapter(format!(
                "取消当前 Step 时获得了非预期结果：{outcome:?}"
            )));
        }
    }
    emit_events(run, execution, port).await?;
    run.begin_step_finalization(step_id)?;
    emit_events(run, execution, port).await?;
    run_step_finalization_phase(
        execution,
        port.persistence_mut(),
        step_id,
        crate::ports::FinalizeCause::UserCancelledStep,
    )
    .await?;
    run.finish_cancelled_step(step_id)?;
    log::debug!(
        target: crate::LOG_TARGET,
        "step cancellation finalization completed: run_id={} step_id={}",
        run.id(),
        step_id
    );
    emit_events(run, execution, port).await
}

pub(super) async fn handle_step_control(
    run: &mut Run,
    execution: &mut RunExecutionState,
    port: &mut RunLoop<'_>,
) -> Result<(), LoopEngineError> {
    match handle_pending_control(run, execution, port).await? {
        Some(ControlDirective::Continue) => Ok(()),
        Some(ControlDirective::Terminal) => Ok(()),
        None => {
            terminate_interrupted_run(run, execution, port).await?;
            Ok(())
        }
    }
}

pub(super) async fn handle_interrupt(
    run: &mut Run,
    execution: &mut RunExecutionState,
    cancel: &CancellationToken,
    port: &mut RunLoop<'_>,
) -> Result<bool, LoopEngineError> {
    if cancel.is_cancelled() {
        terminate_interrupted_run(run, execution, port).await?;
        return Ok(true);
    }
    if run.status().is_terminal() {
        return Ok(true);
    }
    if run.has_timed_out(Instant::now()) {
        timeout_run(run, execution, port).await?;
        return Ok(true);
    }
    Ok(false)
}

pub(super) async fn timeout_run(
    run: &mut Run,
    execution: &mut RunExecutionState,
    port: &mut RunLoop<'_>,
) -> Result<(), LoopEngineError> {
    fail_run(
        run,
        execution,
        port,
        format!(
            "run timed out after {} seconds",
            run.spec().timeout.as_secs()
        ),
    )
    .await
}

pub(crate) async fn fail_run(
    run: &mut Run,
    execution: &mut RunExecutionState,
    port: &mut RunLoop<'_>,
    error: String,
) -> Result<(), LoopEngineError> {
    if run.status().is_terminal() {
        return Ok(());
    }
    run.fail(error)?;
    emit_events(run, execution, port).await
}

pub(super) async fn terminate_interrupted_run(
    run: &mut Run,
    execution: &mut RunExecutionState,
    port: &mut RunLoop<'_>,
) -> Result<(), LoopEngineError> {
    if run.status().is_terminal() {
        return Ok(());
    }
    let active_step = run.active_step_id();
    match run.request_termination(
        sdk::RunTerminationReason::SessionShutdown,
        sdk::ControlDeadline::from_unix_millis(0),
    ) {
        crate::domain::agent_run::RunTerminationRequest::Accepted => {
            emit_events(run, execution, port).await?;
        }
        crate::domain::agent_run::RunTerminationRequest::AlreadyTerminating => {}
        crate::domain::agent_run::RunTerminationRequest::AlreadyTerminal => return Ok(()),
    }
    if let Some(step_id) = active_step {
        run_step_finalization_phase(
            execution,
            port.persistence_mut(),
            &step_id,
            crate::ports::FinalizeCause::RunTerminated,
        )
        .await?;
    }
    run.finish_termination()?;
    emit_events(run, execution, port).await
}

pub(super) async fn transition_and_emit(
    run: &mut Run,
    execution: &mut RunExecutionState,
    port: &mut RunLoop<'_>,
    transition: RunTransition,
) -> Result<(), LoopEngineError> {
    run.transition(transition)?;
    emit_events(run, execution, port).await
}

pub(super) async fn emit_events(
    run: &mut Run,
    execution: &mut RunExecutionState,
    port: &mut RunLoop<'_>,
) -> Result<(), LoopEngineError> {
    let events = run.drain_events();
    if events.is_empty() {
        return Ok(());
    }
    if let Err(error) = port.emit(execution, events.clone()).await {
        run.restore_events(events);
        return Err(error);
    }
    Ok(())
}

pub(super) fn short(id: &sdk::RunId) -> String {
    let s = id.to_string();
    if s.len() > 8 {
        s.split_at(8).0.to_string()
    } else {
        s
    }
}

pub(super) fn model_step_label(step: &ModelStep) -> &'static str {
    match step {
        ModelStep::Complete { .. } => "Complete",
        #[cfg(test)]
        ModelStep::Continue { .. } => "Continue",
        ModelStep::Tools { .. } => "Tools",
    }
}
