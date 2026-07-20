use std::future::Future;
use std::time::Instant;

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

use crate::application::agent::ToolCall;
use crate::domain::agent_run::{
    ModelInvocation, Run, RunCancellationRequest, RunControl, RunDomainEvent, RunStatus,
    RunTransition, RunTransitionError, ToolCallStatus,
};

use super::{StuckDecision, StuckGuard};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoopInput {
    pub text: String,
}

#[derive(Debug, Clone, Default)]
pub struct StepTokenUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cached_tokens: u64,
    pub cache_creation_tokens: u64,
    pub reasoning_tokens: u64,
    pub total_tokens: u64,
    pub context_window: u64,
    /// 估算：system prompt tokens
    pub est_system_tokens: usize,
    /// 估算：tool schemas tokens
    pub est_tool_tokens: usize,
    /// 估算：messages tokens
    pub est_message_tokens: usize,
    /// API 返回的 stop_reason（如 "end_turn" / "max_tokens" / "tool_use"）
    pub stop_reason: String,
}

impl StepTokenUsage {
    /// 估算总量（system + tools + messages）
    pub fn est_total(&self) -> usize {
        self.est_system_tokens + self.est_tool_tokens + self.est_message_tokens
    }
}

#[derive(Clone)]
pub enum ModelStep {
    Complete { text: String },
    Continue { text: String },
    StopHookBlocked { text: String },
    Tools { text: String, calls: Vec<ToolCall> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolGuardDecision {
    Allow,
    SoftBlock { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolStep {
    Continue,
    ContinueWithFuseBypass(Vec<sdk::ToolCallId>),
    AwaitUser,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopDirective {
    Terminal,
    AwaitUser,
}

#[derive(Debug, thiserror::Error)]
pub enum LoopEngineError {
    #[error("run state error: {0}")]
    Domain(#[from] RunTransitionError),
    #[error("loop adapter requested context compaction: {0}")]
    NeedsCompaction(String),
    #[error("loop adapter error: {0}")]
    Adapter(String),
    #[error("loop operation cancelled")]
    Cancelled,
}

#[async_trait]
pub trait RunLoopPort: Send {
    async fn drain_input(&mut self) -> Result<Vec<LoopInput>, LoopEngineError>;
    fn freeze_step(&mut self, _step_id: &sdk::RunStepId, _inputs: &[LoopInput]) {}
    async fn needs_compaction(&mut self) -> Result<bool, LoopEngineError>;
    async fn compact(&mut self, cancel: &CancellationToken) -> Result<(), LoopEngineError>;
    async fn invoke_model(
        &mut self,
        cancel: &CancellationToken,
    ) -> Result<(ModelStep, StepTokenUsage), LoopEngineError>;
    async fn finalize_step(&mut self, _step_id: &sdk::RunStepId) -> Result<(), LoopEngineError> {
        Ok(())
    }
    async fn finalize_cancelled_step(
        &mut self,
        _step_id: &sdk::RunStepId,
    ) -> Result<(), LoopEngineError> {
        Ok(())
    }
    async fn execute_tools(
        &mut self,
        run_id: &sdk::RunId,
        step_id: &sdk::RunStepId,
        calls: &[(ToolCall, ToolGuardDecision)],
        cancel: &CancellationToken,
    ) -> Result<ToolStep, LoopEngineError>;
    async fn on_stuck(&mut self, decision: &StuckDecision) -> Result<(), LoopEngineError>;
    fn claim_terminal(&self, _run_id: &sdk::RunId) -> bool {
        true
    }
    fn claim_cancellation(&self, _run_id: &sdk::RunId) -> bool {
        true
    }
    fn take_control(&mut self, _run_id: &sdk::RunId) -> Option<RunControl> {
        None
    }
    fn take_legacy_cancellation(&mut self, _run_id: &sdk::RunId) -> bool {
        false
    }
    async fn emit(&mut self, events: Vec<RunDomainEvent>) -> Result<(), LoopEngineError>;
}

enum Interrupt<T> {
    Completed(T),
    Cancelled,
    TimedOut,
}

async fn await_interruptible<F, T>(run: &Run, cancel: &CancellationToken, future: F) -> Interrupt<T>
where
    F: Future<Output = T>,
{
    if let Some(remaining) = run.remaining_time(Instant::now()) {
        if remaining.is_zero() {
            return Interrupt::TimedOut;
        }
        tokio::select! {
            biased;
            _ = cancel.cancelled() => Interrupt::Cancelled,
            _ = tokio::time::sleep(remaining) => Interrupt::TimedOut,
            value = future => Interrupt::Completed(value),
        }
    } else {
        tokio::select! {
            biased;
            _ = cancel.cancelled() => Interrupt::Cancelled,
            value = future => Interrupt::Completed(value),
        }
    }
}

pub async fn run_loop<P>(
    run: &mut Run,
    cancel: &CancellationToken,
    port: &mut P,
) -> Result<LoopDirective, LoopEngineError>
where
    P: RunLoopPort,
{
    if run.status() == RunStatus::Created {
        run.transition(RunTransition::Start)?;
        emit_events(run, port).await?;
    }

    log::debug!(
        target: crate::LOG_TARGET,
        "[run_loop] entered run_id={} parent={} spec={:?}",
        run.id(),
        run.parent_id().map(|id| id.to_string()).unwrap_or_else(|| "none".into()),
        run.spec(),
    );

    let mut guard = StuckGuard::new(run.spec().timeout, 5);
    loop {
        match handle_control(run, port).await? {
            ControlDisposition::Terminal => return Ok(LoopDirective::Terminal),
            ControlDisposition::ContinueLoop => continue,
            ControlDisposition::None => {}
        }
        match handle_interrupt(run, cancel, port).await? {
            ControlDisposition::Terminal => return Ok(LoopDirective::Terminal),
            ControlDisposition::ContinueLoop => continue,
            ControlDisposition::None => {}
        }

        let inputs = match await_interruptible(run, cancel, port.drain_input()).await {
            Interrupt::Completed(result) => result?,
            Interrupt::Cancelled => match handle_cancelled_await(run, port).await? {
                ControlDisposition::Terminal => return Ok(LoopDirective::Terminal),
                ControlDisposition::ContinueLoop => continue,
                ControlDisposition::None => {
                    cancel_run(run, port).await?;
                    return Ok(LoopDirective::Terminal);
                }
            },
            Interrupt::TimedOut => {
                timeout_run(run, port).await?;
                return Ok(LoopDirective::Terminal);
            }
        };
        if run.status() == RunStatus::DrainingInput {
            run.apply_drain_decision(if inputs.is_empty() {
                crate::domain::agent_run::DrainDecision::EmptyAndSealed
            } else {
                crate::domain::agent_run::DrainDecision::Inputs
            })?;
            emit_events(run, port).await?;
            if run.is_terminal() {
                return Ok(LoopDirective::Terminal);
            }
        }
        if run.status() == RunStatus::AwaitingUser {
            if inputs.is_empty() {
                return Ok(LoopDirective::AwaitUser);
            }
            run.transition(RunTransition::UserResumed)?;
        }

        let step_id = sdk::RunStepId::new_v7();
        port.freeze_step(&step_id, &inputs);
        let needs_compaction = match await_interruptible(run, cancel, port.needs_compaction()).await
        {
            Interrupt::Completed(result) => result?,
            Interrupt::Cancelled => match handle_cancelled_await(run, port).await? {
                ControlDisposition::Terminal => return Ok(LoopDirective::Terminal),
                ControlDisposition::ContinueLoop => continue,
                ControlDisposition::None => unreachable!("cancelled await always resolves"),
            },
            Interrupt::TimedOut => {
                timeout_run(run, port).await?;
                return Ok(LoopDirective::Terminal);
            }
        };
        if needs_compaction {
            run.transition(RunTransition::BeginCompaction)?;
            match await_interruptible(run, cancel, port.compact(cancel)).await {
                Interrupt::Completed(result) => result?,
                Interrupt::Cancelled => match handle_cancelled_await(run, port).await? {
                    ControlDisposition::Terminal => return Ok(LoopDirective::Terminal),
                    ControlDisposition::ContinueLoop => continue,
                    ControlDisposition::None => unreachable!("cancelled await always resolves"),
                },
                Interrupt::TimedOut => {
                    timeout_run(run, port).await?;
                    return Ok(LoopDirective::Terminal);
                }
            }
            run.transition(RunTransition::CompactionCompleted)?;
        }

        match handle_interrupt(run, cancel, port).await? {
            ControlDisposition::Terminal => return Ok(LoopDirective::Terminal),
            ControlDisposition::ContinueLoop => continue,
            ControlDisposition::None => {}
        }
        run.transition(RunTransition::ContextPrepared)?;
        let step_id = run.begin_step_with_id(step_id)?;
        emit_events(run, port).await?;
        let mut compacted_after_context_too_long = false;
        let (model_step, token_usage) = loop {
            match await_interruptible(run, cancel, port.invoke_model(cancel)).await {
                Interrupt::Completed(Ok(result)) => break result,
                Interrupt::Completed(Err(LoopEngineError::Cancelled)) | Interrupt::Cancelled => {
                    match handle_cancelled_await(run, port).await? {
                        ControlDisposition::Terminal => return Ok(LoopDirective::Terminal),
                        ControlDisposition::ContinueLoop => continue,
                        ControlDisposition::None => unreachable!("cancelled await always resolves"),
                    }
                }
                Interrupt::Completed(Err(LoopEngineError::NeedsCompaction(error))) => {
                    if compacted_after_context_too_long {
                        fail_run(
                            run,
                            port,
                            format!("compact 后 Provider 仍报告 context 超限：{error}"),
                        )
                        .await?;
                        return Ok(LoopDirective::Terminal);
                    }
                    run.transition(RunTransition::ModelContextExceeded)?;
                    match await_interruptible(run, cancel, port.compact(cancel)).await {
                        Interrupt::Completed(result) => result?,
                        Interrupt::Cancelled => match handle_cancelled_await(run, port).await? {
                            ControlDisposition::Terminal => return Ok(LoopDirective::Terminal),
                            ControlDisposition::ContinueLoop => continue,
                            ControlDisposition::None => {
                                unreachable!("cancelled await always resolves")
                            }
                        },
                        Interrupt::TimedOut => {
                            timeout_run(run, port).await?;
                            return Ok(LoopDirective::Terminal);
                        }
                    }
                    run.transition(RunTransition::CompactionCompleted)?;
                    run.transition(RunTransition::ContextPrepared)?;
                    compacted_after_context_too_long = true;
                }
                Interrupt::Completed(Err(error)) => {
                    fail_run(run, port, error.to_string()).await?;
                    return Ok(LoopDirective::Terminal);
                }
                Interrupt::TimedOut => {
                    timeout_run(run, port).await?;
                    return Ok(LoopDirective::Terminal);
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
        match handle_interrupt(run, cancel, port).await? {
            ControlDisposition::Terminal => return Ok(LoopDirective::Terminal),
            ControlDisposition::ContinueLoop => continue,
            ControlDisposition::None => {}
        }
        run.record_model_invocation(&step_id, model_invocation(&model_step))?;
        run.transition(RunTransition::ModelInvoked)?;
        log::debug!(
            target: crate::LOG_TARGET,
            "[run_loop] model_step={} run_id={}",
            model_step_label(&model_step),
            short(run.id()),
        );

        match handle_control(run, port).await? {
            ControlDisposition::Terminal => return Ok(LoopDirective::Terminal),
            ControlDisposition::ContinueLoop => continue,
            ControlDisposition::None => {}
        }
        match model_step {
            ModelStep::Complete { text } => {
                match guard.inspect_text(&text) {
                    decision @ StuckDecision::SoftBlock { .. } => {
                        record_stuck(run, port, &decision).await?;
                        run.transition(RunTransition::ContinueAfterResponse)?;
                        run.complete_step(&step_id)?;
                        port.finalize_step(&step_id).await?;
                        continue;
                    }
                    decision @ StuckDecision::HardPause { .. } => {
                        let reason = match &decision {
                            StuckDecision::HardPause { reason } => reason.clone(),
                            _ => unreachable!(),
                        };
                        record_stuck(run, port, &decision).await?;
                        fail_run(run, port, reason).await?;
                        return Ok(LoopDirective::Terminal);
                    }
                    StuckDecision::Allow | StuckDecision::Fail { .. } => {}
                }
                run.transition(RunTransition::ResponseWithoutTools)?;
                run.complete_step(&step_id)?;
                port.finalize_step(&step_id).await?;
                match handle_interrupt(run, cancel, port).await? {
                    ControlDisposition::Terminal => return Ok(LoopDirective::Terminal),
                    ControlDisposition::ContinueLoop => continue,
                    ControlDisposition::None => {}
                }
                if !port.claim_terminal(run.id()) {
                    cancel_run(run, port).await?;
                    return Ok(LoopDirective::Terminal);
                }
                run.complete(text)?;
                emit_events(run, port).await?;
                return Ok(LoopDirective::Terminal);
            }
            ModelStep::Continue { text } => {
                let decision = guard.inspect_text(&text);
                match decision {
                    StuckDecision::SoftBlock { .. } => record_stuck(run, port, &decision).await?,
                    StuckDecision::HardPause { ref reason } => {
                        let reason = reason.clone();
                        record_stuck(run, port, &decision).await?;
                        fail_run(run, port, reason).await?;
                        return Ok(LoopDirective::Terminal);
                    }
                    StuckDecision::Allow | StuckDecision::Fail { .. } => {}
                }
                run.transition(RunTransition::ContinueAfterResponse)?;
                run.complete_step(&step_id)?;
                port.finalize_step(&step_id).await?;
            }
            ModelStep::StopHookBlocked { text: _ } => {
                let decision = guard.record_stop_hook_block();
                record_stuck(run, port, &decision).await?;
                match decision {
                    StuckDecision::Fail { reason } => {
                        fail_run(run, port, reason).await?;
                        return Ok(LoopDirective::Terminal);
                    }
                    StuckDecision::Allow
                    | StuckDecision::SoftBlock { .. }
                    | StuckDecision::HardPause { .. } => {
                        run.transition(RunTransition::ContinueAfterResponse)?;
                        run.complete_step(&step_id)?;
                        // Stop Hook Block 明确不提交当前 Step。
                    }
                }
            }
            ModelStep::Tools { text, calls } => {
                if let decision @ StuckDecision::SoftBlock { .. } = guard.inspect_text(&text) {
                    record_stuck(run, port, &decision).await?;
                }
                run.transition(RunTransition::ResponseWithTools)?;
                let mut guarded_calls = Vec::with_capacity(calls.len());
                for call in calls {
                    run.add_tool_call(&step_id, call.clone())?;
                    match guard.inspect_tool(&call) {
                        StuckDecision::SoftBlock { reason } => {
                            record_stuck(
                                run,
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
                                port,
                                &StuckDecision::HardPause {
                                    reason: reason.clone(),
                                },
                            )
                            .await?;
                            fail_run(run, port, reason).await?;
                            return Ok(LoopDirective::Terminal);
                        }
                        StuckDecision::Allow | StuckDecision::Fail { .. } => {
                            guarded_calls.push((call, ToolGuardDecision::Allow));
                        }
                    }
                }
                for (call, _) in &guarded_calls {
                    run.advance_tool_call(&step_id, &call.id, ToolCallStatus::Ready)?;
                }
                run.transition(RunTransition::ToolsApproved)?;
                for (call, decision) in &guarded_calls {
                    if matches!(decision, ToolGuardDecision::Allow) {
                        run.advance_tool_call(&step_id, &call.id, ToolCallStatus::Running)?;
                    }
                }
                log::debug!(
                    target: crate::LOG_TARGET,
                    "[run_loop] execute_tools count={} run_id={}",
                    guarded_calls.len(),
                    short(run.id()),
                );
                let tool_step = match await_interruptible(
                    run,
                    cancel,
                    port.execute_tools(run.id(), &step_id, &guarded_calls, cancel),
                )
                .await
                {
                    Interrupt::Completed(Ok(step)) => step,
                    Interrupt::Completed(Err(LoopEngineError::Cancelled))
                    | Interrupt::Cancelled => match handle_cancelled_await(run, port).await? {
                        ControlDisposition::Terminal => return Ok(LoopDirective::Terminal),
                        ControlDisposition::ContinueLoop => continue,
                        ControlDisposition::None => unreachable!("cancelled await always resolves"),
                    },
                    Interrupt::Completed(Err(error)) => {
                        fail_run(run, port, error.to_string()).await?;
                        return Ok(LoopDirective::Terminal);
                    }
                    Interrupt::TimedOut => {
                        timeout_run(run, port).await?;
                        return Ok(LoopDirective::Terminal);
                    }
                };
                match handle_interrupt(run, cancel, port).await? {
                    ControlDisposition::Terminal => return Ok(LoopDirective::Terminal),
                    ControlDisposition::ContinueLoop => continue,
                    ControlDisposition::None => {}
                }
                let fuse_bypassed = match &tool_step {
                    ToolStep::ContinueWithFuseBypass(ids) => ids.as_slice(),
                    ToolStep::Continue | ToolStep::AwaitUser => &[],
                };
                for (call, decision) in &guarded_calls {
                    let bypassed = fuse_bypassed.contains(&call.id);
                    let status = if matches!(decision, ToolGuardDecision::Allow) || bypassed {
                        ToolCallStatus::Success
                    } else {
                        ToolCallStatus::Cancelled
                    };
                    run.advance_tool_call(&step_id, &call.id, status)?;
                }
                match tool_step {
                    ToolStep::Continue | ToolStep::ContinueWithFuseBypass(_) => {
                        run.complete_step(&step_id)?;
                        port.finalize_step(&step_id).await?;
                        run.transition(RunTransition::ToolsCompleted)?;
                    }
                    ToolStep::AwaitUser => {
                        run.complete_step(&step_id)?;
                        run.transition(RunTransition::AwaitUser)?;
                        emit_events(run, port).await?;
                        return Ok(LoopDirective::AwaitUser);
                    }
                }
            }
        }
    }
}

fn model_invocation(step: &ModelStep) -> ModelInvocation {
    let response = match step {
        ModelStep::Complete { text }
        | ModelStep::Continue { text }
        | ModelStep::StopHookBlocked { text }
        | ModelStep::Tools { text, .. } => text.clone(),
    };
    ModelInvocation::new("", response)
}

async fn record_stuck<P>(
    run: &mut Run,
    port: &mut P,
    decision: &StuckDecision,
) -> Result<(), LoopEngineError>
where
    P: RunLoopPort,
{
    let reason = match decision {
        StuckDecision::SoftBlock { reason }
        | StuckDecision::HardPause { reason }
        | StuckDecision::Fail { reason } => reason.clone(),
        StuckDecision::Allow => return Ok(()),
    };
    run.mark_stuck(reason)?;
    emit_events(run, port).await?;
    port.on_stuck(decision).await
}

enum ControlDisposition {
    None,
    ContinueLoop,
    Terminal,
}

async fn handle_cancelled_await<P>(
    run: &mut Run,
    port: &mut P,
) -> Result<ControlDisposition, LoopEngineError>
where
    P: RunLoopPort,
{
    match handle_control(run, port).await? {
        ControlDisposition::None if port.take_legacy_cancellation(run.id()) => {
            cancel_run(run, port).await?;
            Ok(ControlDisposition::Terminal)
        }
        ControlDisposition::None => {
            cancel_run(run, port).await?;
            Ok(ControlDisposition::Terminal)
        }
        disposition => Ok(disposition),
    }
}

async fn handle_control<P>(
    run: &mut Run,
    port: &mut P,
) -> Result<ControlDisposition, LoopEngineError>
where
    P: RunLoopPort,
{
    let Some(control) = port.take_control(run.id()) else {
        return Ok(ControlDisposition::None);
    };
    match control {
        RunControl::CancelStep => {
            let Some(step_id) = run.active_step_id() else {
                return Ok(ControlDisposition::None);
            };
            match run.request_step_cancellation(&step_id) {
                crate::domain::agent_run::RunStepCancellationRequest::Accepted => {
                    emit_events(run, port).await?;
                    run.begin_step_finalization(&step_id)?;
                    emit_events(run, port).await?;
                    port.finalize_cancelled_step(&step_id).await?;
                    run.finish_cancelled_step(&step_id)?;
                    emit_events(run, port).await?;
                    Ok(ControlDisposition::ContinueLoop)
                }
                crate::domain::agent_run::RunStepCancellationRequest::AlreadyCancelling => {
                    Ok(ControlDisposition::ContinueLoop)
                }
                crate::domain::agent_run::RunStepCancellationRequest::NoActiveStep => {
                    Ok(ControlDisposition::None)
                }
                crate::domain::agent_run::RunStepCancellationRequest::RunTerminating
                | crate::domain::agent_run::RunStepCancellationRequest::RunTerminal => {
                    Ok(ControlDisposition::Terminal)
                }
            }
        }
        RunControl::Terminate { reason, deadline } => {
            match run.request_termination(reason, deadline) {
                crate::domain::agent_run::RunTerminationRequest::Accepted => {
                    emit_events(run, port).await?;
                    let finalization_error = if let Some(step_id) = run.active_step_id() {
                        port.finalize_cancelled_step(&step_id).await.err()
                    } else {
                        None
                    };
                    run.finish_termination()?;
                    emit_events(run, port).await?;
                    if let Some(error) = finalization_error {
                        return Err(error);
                    }
                }
                crate::domain::agent_run::RunTerminationRequest::AlreadyTerminating => {}
                crate::domain::agent_run::RunTerminationRequest::AlreadyTerminal => {}
            }
            Ok(ControlDisposition::Terminal)
        }
    }
}

async fn handle_interrupt<P>(
    run: &mut Run,
    cancel: &CancellationToken,
    port: &mut P,
) -> Result<ControlDisposition, LoopEngineError>
where
    P: RunLoopPort,
{
    if cancel.is_cancelled() || run.status() == RunStatus::Cancelling {
        match handle_control(run, port).await? {
            ControlDisposition::None if port.take_legacy_cancellation(run.id()) => {
                cancel_run(run, port).await?;
                return Ok(ControlDisposition::Terminal);
            }
            ControlDisposition::None => return Ok(ControlDisposition::None),
            disposition => return Ok(disposition),
        }
    }
    if run.has_timed_out(Instant::now()) {
        timeout_run(run, port).await?;
        return Ok(ControlDisposition::Terminal);
    }
    Ok(ControlDisposition::None)
}

async fn timeout_run<P>(run: &mut Run, port: &mut P) -> Result<(), LoopEngineError>
where
    P: RunLoopPort,
{
    fail_run(
        run,
        port,
        format!(
            "run timed out after {} seconds",
            run.spec().timeout.as_secs()
        ),
    )
    .await
}

async fn fail_run<P>(run: &mut Run, port: &mut P, error: String) -> Result<(), LoopEngineError>
where
    P: RunLoopPort,
{
    if !port.claim_terminal(run.id()) {
        return cancel_run(run, port).await;
    }
    run.fail(error)?;
    emit_events(run, port).await
}

async fn cancel_run<P>(run: &mut Run, port: &mut P) -> Result<(), LoopEngineError>
where
    P: RunLoopPort,
{
    let active_step = run.active_step_id();
    if run.status() != RunStatus::Cancelling {
        if !port.claim_cancellation(run.id()) {
            log::debug!(
                target: crate::LOG_TARGET,
                "[cancel_run] cancellation not claimed (owned by another port) run_id={}",
                short(run.id()),
            );
            return Ok(());
        }
        match run.request_cancellation() {
            RunCancellationRequest::Accepted | RunCancellationRequest::AlreadyCancelling => {}
            RunCancellationRequest::AlreadyTerminal => return Ok(()),
        }
        log::debug!(
            target: crate::LOG_TARGET,
            "[cancel_run] phase1 CancellationRequested run_id={}",
            short(run.id()),
        );
        emit_events(run, port).await?;
    }
    log::debug!(
        target: crate::LOG_TARGET,
        "[cancel_run] phase2 finish_cancellation run_id={}",
        short(run.id()),
    );
    if let Some(step_id) = &active_step {
        port.finalize_cancelled_step(step_id).await?;
    }
    run.finish_cancellation()?;
    emit_events(run, port).await
}

async fn emit_events<P>(run: &mut Run, port: &mut P) -> Result<(), LoopEngineError>
where
    P: RunLoopPort,
{
    let events = run.drain_events();
    if events.is_empty() {
        return Ok(());
    }
    if let Err(error) = port.emit(events.clone()).await {
        run.restore_events(events);
        return Err(error);
    }
    Ok(())
}

fn short(id: &sdk::RunId) -> String {
    let s = id.to_string();
    if s.len() > 8 {
        s.split_at(8).0.to_string()
    } else {
        s
    }
}

fn model_step_label(step: &ModelStep) -> &'static str {
    match step {
        ModelStep::Complete { .. } => "Complete",
        ModelStep::Continue { .. } => "Continue",
        ModelStep::StopHookBlocked { .. } => "StopHookBlocked",
        ModelStep::Tools { .. } => "Tools",
    }
}
