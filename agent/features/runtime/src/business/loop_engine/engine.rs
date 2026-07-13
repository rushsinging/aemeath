use std::future::Future;
use std::time::Instant;

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

use crate::business::agent::ToolCall;
use crate::business::agent_run::{
    ModelInvocation, Run, RunCancellationRequest, RunDomainEvent, RunStatus, RunTransition,
    RunTransitionError, ToolCallStatus,
};

use super::{StuckDecision, StuckGuard};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoopInput {
    pub text: String,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolStep {
    Continue,
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
    #[error("loop adapter error: {0}")]
    Adapter(String),
    #[error("loop operation cancelled")]
    Cancelled,
}

#[async_trait]
pub trait RunLoopPort: Send {
    async fn drain_input(&mut self) -> Result<Vec<LoopInput>, LoopEngineError>;
    async fn needs_compaction(&mut self) -> Result<bool, LoopEngineError>;
    async fn compact(&mut self, cancel: &CancellationToken) -> Result<(), LoopEngineError>;
    async fn invoke_model(
        &mut self,
        cancel: &CancellationToken,
    ) -> Result<ModelStep, LoopEngineError>;
    async fn execute_tools(
        &mut self,
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
        target: "aemeath:agent:runtime",
        "[run_loop] entered run_id={} parent={} spec={:?}",
        run.id(),
        run.parent_id().map(|id| id.to_string()).unwrap_or_else(|| "none".into()),
        run.spec(),
    );

    let mut guard = StuckGuard::new(run.spec().timeout, 5);
    loop {
        if handle_interrupt(run, cancel, port).await? {
            return Ok(LoopDirective::Terminal);
        }

        let inputs = match await_interruptible(run, cancel, port.drain_input()).await {
            Interrupt::Completed(result) => result?,
            Interrupt::Cancelled => {
                cancel_run(run, port).await?;
                return Ok(LoopDirective::Terminal);
            }
            Interrupt::TimedOut => {
                timeout_run(run, port).await?;
                return Ok(LoopDirective::Terminal);
            }
        };
        if run.status() == RunStatus::AwaitingUser {
            if inputs.is_empty() {
                return Ok(LoopDirective::AwaitUser);
            }
            run.transition(RunTransition::UserResumed)?;
        }

        let needs_compaction = match await_interruptible(run, cancel, port.needs_compaction()).await
        {
            Interrupt::Completed(result) => result?,
            Interrupt::Cancelled => {
                cancel_run(run, port).await?;
                return Ok(LoopDirective::Terminal);
            }
            Interrupt::TimedOut => {
                timeout_run(run, port).await?;
                return Ok(LoopDirective::Terminal);
            }
        };
        if needs_compaction {
            run.transition(RunTransition::BeginCompaction)?;
            match await_interruptible(run, cancel, port.compact(cancel)).await {
                Interrupt::Completed(result) => result?,
                Interrupt::Cancelled => {
                    cancel_run(run, port).await?;
                    return Ok(LoopDirective::Terminal);
                }
                Interrupt::TimedOut => {
                    timeout_run(run, port).await?;
                    return Ok(LoopDirective::Terminal);
                }
            }
            run.transition(RunTransition::CompactionCompleted)?;
        }

        if handle_interrupt(run, cancel, port).await? {
            return Ok(LoopDirective::Terminal);
        }
        run.transition(RunTransition::ContextPrepared)?;
        let step_id = run.begin_step()?;
        emit_events(run, port).await?;
        let model_step = match await_interruptible(run, cancel, port.invoke_model(cancel)).await {
            Interrupt::Completed(Ok(step)) => step,
            Interrupt::Completed(Err(LoopEngineError::Cancelled)) | Interrupt::Cancelled => {
                cancel_run(run, port).await?;
                return Ok(LoopDirective::Terminal);
            }
            Interrupt::Completed(Err(error)) => {
                fail_run(run, port, error.to_string()).await?;
                return Ok(LoopDirective::Terminal);
            }
            Interrupt::TimedOut => {
                timeout_run(run, port).await?;
                return Ok(LoopDirective::Terminal);
            }
        };
        if handle_interrupt(run, cancel, port).await? {
            return Ok(LoopDirective::Terminal);
        }
        run.record_model_invocation(&step_id, model_invocation(&model_step))?;
        run.transition(RunTransition::ModelInvoked)?;
        log::debug!(
            target: "aemeath:agent:runtime",
            "[run_loop] model_step={} run_id={}",
            model_step_label(&model_step),
            short(run.id()),
        );

        match model_step {
            ModelStep::Complete { text } => {
                match guard.inspect_text(&text) {
                    decision @ StuckDecision::SoftBlock { .. } => {
                        record_stuck(run, port, &decision).await?;
                        run.transition(RunTransition::ContinueAfterResponse)?;
                        run.complete_step(&step_id)?;
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
                if handle_interrupt(run, cancel, port).await? {
                    return Ok(LoopDirective::Terminal);
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
                for (call, decision) in &guarded_calls {
                    let status = match decision {
                        ToolGuardDecision::Allow => ToolCallStatus::Ready,
                        ToolGuardDecision::SoftBlock { .. } => ToolCallStatus::Cancelled,
                    };
                    run.advance_tool_call(&step_id, &call.id, status)?;
                }
                run.transition(RunTransition::ToolsApproved)?;
                for (call, decision) in &guarded_calls {
                    if matches!(decision, ToolGuardDecision::Allow) {
                        run.advance_tool_call(&step_id, &call.id, ToolCallStatus::Running)?;
                    }
                }
                log::debug!(
                    target: "aemeath:agent:runtime",
                    "[run_loop] execute_tools count={} run_id={}",
                    guarded_calls.len(),
                    short(run.id()),
                );
                let tool_step = match await_interruptible(
                    run,
                    cancel,
                    port.execute_tools(&guarded_calls, cancel),
                )
                .await
                {
                    Interrupt::Completed(Ok(step)) => step,
                    Interrupt::Completed(Err(LoopEngineError::Cancelled))
                    | Interrupt::Cancelled => {
                        cancel_run(run, port).await?;
                        return Ok(LoopDirective::Terminal);
                    }
                    Interrupt::Completed(Err(error)) => {
                        fail_run(run, port, error.to_string()).await?;
                        return Ok(LoopDirective::Terminal);
                    }
                    Interrupt::TimedOut => {
                        timeout_run(run, port).await?;
                        return Ok(LoopDirective::Terminal);
                    }
                };
                if handle_interrupt(run, cancel, port).await? {
                    return Ok(LoopDirective::Terminal);
                }
                for (call, decision) in &guarded_calls {
                    if matches!(decision, ToolGuardDecision::Allow) {
                        run.advance_tool_call(&step_id, &call.id, ToolCallStatus::Success)?;
                    }
                }
                match tool_step {
                    ToolStep::Continue => {
                        run.complete_step(&step_id)?;
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

async fn handle_interrupt<P>(
    run: &mut Run,
    cancel: &CancellationToken,
    port: &mut P,
) -> Result<bool, LoopEngineError>
where
    P: RunLoopPort,
{
    if cancel.is_cancelled() || run.status() == RunStatus::Cancelling {
        cancel_run(run, port).await?;
        return Ok(true);
    }
    if run.has_timed_out(Instant::now()) {
        timeout_run(run, port).await?;
        return Ok(true);
    }
    Ok(false)
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
    if run.status() != RunStatus::Cancelling {
        if !port.claim_cancellation(run.id()) {
            log::debug!(
                target: "aemeath:agent:runtime",
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
            target: "aemeath:agent:runtime",
            "[cancel_run] phase1 CancellationRequested run_id={}",
            short(run.id()),
        );
        emit_events(run, port).await?;
    }
    log::debug!(
        target: "aemeath:agent:runtime",
        "[cancel_run] phase2 finish_cancellation run_id={}",
        short(run.id()),
    );
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
    for event in &events {
        log::debug!(
            target: "aemeath:agent:runtime",
            "[run_domain] {} run_id={} parent={}",
            event_name(event),
            event_short_id(event),
            event.parent_run_id().map(|id| short(id)).unwrap_or_else(|| "none".into()),
        );
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

fn event_name(event: &RunDomainEvent) -> &'static str {
    match event {
        RunDomainEvent::Started { .. } => "Started",
        RunDomainEvent::StepStarted { .. } => "StepStarted",
        RunDomainEvent::StepCompleted { .. } => "StepCompleted",
        RunDomainEvent::CancellationRequested { .. } => "CancellationRequested",
        RunDomainEvent::AwaitingUser { .. } => "AwaitingUser",
        RunDomainEvent::Resumed { .. } => "Resumed",
        RunDomainEvent::StuckDetected { .. } => "StuckDetected",
        RunDomainEvent::Completed { .. } => "Completed",
        RunDomainEvent::Failed { .. } => "Failed",
        RunDomainEvent::Cancelled { .. } => "Cancelled",
    }
}

fn event_short_id(event: &RunDomainEvent) -> String {
    let id = match event {
        RunDomainEvent::Started { run_id, .. }
        | RunDomainEvent::StepStarted { run_id, .. }
        | RunDomainEvent::StepCompleted { run_id, .. }
        | RunDomainEvent::CancellationRequested { run_id, .. }
        | RunDomainEvent::AwaitingUser { run_id, .. }
        | RunDomainEvent::Resumed { run_id, .. }
        | RunDomainEvent::StuckDetected { run_id, .. }
        | RunDomainEvent::Completed { run_id, .. }
        | RunDomainEvent::Failed { run_id, .. }
        | RunDomainEvent::Cancelled { run_id, .. } => run_id,
    };
    short(id)
}

fn model_step_label(step: &ModelStep) -> &'static str {
    match step {
        ModelStep::Complete { .. } => "Complete",
        ModelStep::Continue { .. } => "Continue",
        ModelStep::StopHookBlocked { .. } => "StopHookBlocked",
        ModelStep::Tools { .. } => "Tools",
    }
}
