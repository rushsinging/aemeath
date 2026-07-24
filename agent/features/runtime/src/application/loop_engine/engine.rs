use std::future::Future;
use std::time::Instant;

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

use crate::application::subagent::ToolCall;
use crate::domain::agent_run::{
    DrainDecision, ModelInvocation, Run, RunCancellationRequest, RunDomainEvent, RunStatus,
    RunTransition, RunTransitionError, ToolCallStatus,
};

use super::{StuckDecision, StuckGuard};

/// Monotonic per-Run drain epoch. Each successful drain call increments
/// the epoch. Callers pass their expected epoch for mismatch detection
/// (#1272 per-turn drain-or-seal linearization).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DrainEpoch(pub u64);

impl DrainEpoch {
    /// Advance to the next epoch.
    pub fn next(self) -> Self {
        Self(self.0 + 1)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoopInput {
    pub text: String,
    /// Per-turn user message InputId (from `ChatInputEvent::UserMessage::id`).
    /// `None` for engine-driven continuations (StopHookFeedback, ToolResults)
    /// and fixed-sub-agent prompts (#1272 per-turn drain identity).
    pub input_id: Option<sdk::InputId>,
    /// Per-turn user message images (from `ChatInputEvent::UserMessage::images`).
    /// Empty for engine-driven continuations.
    pub images: Vec<sdk::ChatInputImage>,
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

/// Returned by `drain_input` to tell the engine what to do next.
///
/// #1272 Per-turn drain-or-seal contract:
/// - `Ready` carries a **non-empty** batch of user input.
/// - `InternalContinuation` is for engine-driven continuations: stop hook
///   feedback or recorded tool results. The batch can be empty (pure
///   continuation) or carry any user input that arrived alongside it.
/// - `EmptyAndSealed` is the unique terminal gate.
///
/// Each variant carries a [`DrainEpoch`] for per-turn linearization.
#[derive(Debug, Clone)]
pub enum DrainOutcome {
    /// User input is ready for the next step. The batch SHOULD be non-empty;
    /// use [`DrainOutcome::ready`] to construct. An empty batch is detected
    /// by `run_loop` and reported as `LoopEngineError::Adapter` (#1272).
    Ready {
        batch: Vec<LoopInput>,
        epoch: DrainEpoch,
    },
    /// Engine-driven continuation (stop-hook feedback or tool results).
    InternalContinuation {
        kind: InternalContinuationKind,
        batch: Vec<LoopInput>,
        epoch: DrainEpoch,
    },
    /// No more work: seal the Run and transition to Completed.
    EmptyAndSealed { epoch: DrainEpoch },
    /// No user input available while awaiting user. Buffer is not sealed;
    /// epoch is not advanced. Caller should return AwaitUser and retry
    /// with the same expected epoch (#1272).
    NoInput { epoch: DrainEpoch },
}

impl DrainOutcome {
    /// Construct a `Ready` outcome. Does not panic on empty batch — an
    /// empty `Ready` is detected by `run_loop` at the shared consumption
    /// point and reported as `LoopEngineError::Adapter` (#1272 close-out).
    /// Adapters should still avoid producing empty Ready; for no-work seal
    /// use `DrainOutcome::EmptyAndSealed` directly.
    pub fn ready(batch: Vec<LoopInput>, epoch: DrainEpoch) -> Self {
        Self::Ready { batch, epoch }
    }

    /// The epoch carried by this outcome — used by the engine to validate
    /// per-turn linearization (#1272).
    pub fn epoch(&self) -> DrainEpoch {
        match self {
            Self::Ready { epoch, .. }
            | Self::InternalContinuation { epoch, .. }
            | Self::EmptyAndSealed { epoch }
            | Self::NoInput { epoch } => *epoch,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InternalContinuationKind {
    /// Stop hook produced feedback; the model should see it as a system prefix.
    StopHookFeedback { feedback: String },
    /// Tool results have been recorded; the model should read them.
    ToolResults,
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
    async fn drain_input(
        &mut self,
        expected_epoch: DrainEpoch,
    ) -> Result<DrainOutcome, LoopEngineError>;
    /// #1272: Drain input while the Run is AwaitingUser. Unlike
    /// `drain_input`, this must NOT seal the input buffer or advance
    /// epoch when no user input is available — the buffer must stay
    /// receptive to future input within the same Run.
    ///
    /// The default impl returns an `Adapter` error: an adapter that can
    /// reach `AwaitingUser` MUST override this to ensure empty input
    /// returns `NoInput` (not `EmptyAndSealed`) and never seals the buffer.
    /// Adapters that never enter `AwaitingUser` (e.g. Sub agents with a
    /// fixed prompt) do not need to override this.
    async fn await_user_input(
        &mut self,
        expected_epoch: DrainEpoch,
    ) -> Result<DrainOutcome, LoopEngineError> {
        log::debug!(
            target: crate::LOG_TARGET,
            "RunLoopPort::await_user_input 使用默认实现（epoch {:?}）：\
             该 adapter 未覆写 await_user_input，无法安全处理 AwaitingUser",
            expected_epoch,
        );
        Err(LoopEngineError::Adapter(format!(
            "该 adapter 未覆写 await_user_input（epoch {:?}）：\
             可进入 AwaitingUser 的 adapter 必须实现该方法，\
             保证空输入时返回 NoInput 而非 seal buffer",
            expected_epoch,
        )))
    }
    fn freeze_step(&mut self, _step_id: &sdk::RunStepId, _inputs: &[LoopInput]) {}
    async fn accept_step_input(
        &mut self,
        _step_id: &sdk::RunStepId,
    ) -> Result<(), LoopEngineError> {
        Ok(())
    }
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
    fn take_control(&self, _run_id: &sdk::RunId) -> Option<crate::domain::agent_run::RunControl> {
        None
    }
    fn register_step_scope(
        &self,
        _run_id: &sdk::RunId,
        _step_id: sdk::RunStepId,
        _cancel: CancellationToken,
    ) {
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
        run.start_draining()?;
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
    // #1272: engine-owned epoch for per-turn drain linearization.
    // Initialized from the Run's persisted epoch so that re-entering
    // run_loop (e.g. after AwaitUser) recovers the correct epoch
    // instead of resetting to 0.  Each successful drain increments
    // both the engine-local counter and the Run's persisted epoch.
    let mut expected_epoch = DrainEpoch(run.next_drain_epoch());
    // #1272: collect the last assistant text for terminal claim across
    // loop iterations. Every model step response is tracked; the last
    // one before EmptyAndSealed becomes the terminal text carried in the
    // Completed event. Must live outside the loop block — otherwise
    // Complete→drain→EmptyAndSealed loses the result.
    let mut terminal_text: Option<String> = None;
    loop {
        if let Some(control) = handle_pending_control(run, port).await? {
            if matches!(control, ControlDirective::Terminal) {
                return Ok(LoopDirective::Terminal);
            }
            continue;
        }
        if handle_interrupt(run, cancel, port).await? {
            return Ok(LoopDirective::Terminal);
        }
        // #1272: failed/cancelled runs are terminal; do not drain again.
        if run.status().is_terminal() {
            return Ok(LoopDirective::Terminal);
        }

        // ---- drain phase ----
        // #1272: When AwaitingUser, use await_user_input which never
        // seals the input buffer on empty — the buffer stays receptive
        // to future user input in the same Run.
        let awaiting_user = run.status() == RunStatus::AwaitingUser;
        let drain_future = if awaiting_user {
            port.await_user_input(expected_epoch)
        } else {
            port.drain_input(expected_epoch)
        };
        let outcome = match await_interruptible(run, cancel, drain_future).await {
            Interrupt::Completed(result) => result?,
            Interrupt::Cancelled => {
                if let Some(control) = handle_pending_control(run, port).await? {
                    return Ok(match control {
                        ControlDirective::Continue => LoopDirective::AwaitUser,
                        ControlDirective::Terminal => LoopDirective::Terminal,
                    });
                }
                cancel_run(run, port).await?;
                return Ok(LoopDirective::Terminal);
            }
            Interrupt::TimedOut => {
                timeout_run(run, port).await?;
                return Ok(LoopDirective::Terminal);
            }
        };

        // #1272: validate that the adapter returned the epoch the engine expects.
        if outcome.epoch() != expected_epoch {
            return Err(LoopEngineError::Adapter(format!(
                "drain epoch 不匹配：期望 {:?}，实际 {:?}",
                expected_epoch,
                outcome.epoch(),
            )));
        }

        match outcome {
            DrainOutcome::Ready { batch, .. } => {
                // #1272 close-out: an empty Ready batch is a contract
                // violation (Ready must carry non-empty user input).
                // Detect it here — before any epoch advance or state
                // transition — and return a descriptive Adapter error
                // instead of panicking.
                if batch.is_empty() {
                    log::error!(
                        target: crate::LOG_TARGET,
                        "[run_loop] adapter 返回了空 Ready batch（epoch {:?}），\
                         这违反了 Ready 必须携带非空用户输入的契约",
                        expected_epoch,
                    );
                    return Err(LoopEngineError::Adapter(format!(
                        "drain_or_seal 在 epoch {:?} 返回了空的 Ready batch：\
                         Ready 必须携带非空用户输入，请改用 EmptyAndSealed 或 NoInput",
                        expected_epoch,
                    )));
                }
                // #1272: advance epoch BEFORE apply_drain_decision so that
                // epoch is incremented even if the decision fails (the
                // buffer already advanced its epoch; keeping them in sync
                // prevents a poisoned epoch on failure retry).
                run.advance_drain_epoch();
                expected_epoch = expected_epoch.next();

                // User input: resume if awaiting, then drain into work.
                if run.status() == RunStatus::AwaitingUser {
                    run.transition(RunTransition::UserResumed)?;
                }
                // batch is non-empty per DrainOutcome::Ready contract
                run.apply_drain_decision(DrainDecision::Inputs, None)?;
                execute_step(run, cancel, port, &mut guard, &batch, &mut terminal_text).await?;
            }
            DrainOutcome::InternalContinuation {
                kind: _kind, batch, ..
            } => {
                // #1272: InternalContinuation always advances epoch because
                // `take_internal_continuation` already advanced the buffer's
                // epoch — the continuation itself is a drain event, even
                // when the batch is empty.
                run.advance_drain_epoch();
                expected_epoch = expected_epoch.next();

                if run.status() == RunStatus::AwaitingUser {
                    // #1272: InternalContinuation without user input while
                    // awaiting user — do not auto-resume.  Only Ready
                    // (which guarantees a non-empty batch) resumes from
                    // AwaitingUser.  Return AwaitUser (epoch already
                    // advanced for the continuation).
                    if batch.is_empty() {
                        return Ok(LoopDirective::AwaitUser);
                    }
                    run.transition(RunTransition::UserResumed)?;
                }
                run.apply_drain_decision(DrainDecision::InternalContinuation, None)?;
                execute_step(run, cancel, port, &mut guard, &batch, &mut terminal_text).await?;
            }
            DrainOutcome::NoInput { .. } => {
                // #1272: NoInput from await_user_input — buffer is NOT
                // sealed, epoch is NOT advanced. Return AwaitUser so the
                // caller can wait for user input and re-enter with the
                // same expected epoch.
                debug_assert!(
                    awaiting_user,
                    "NoInput should only be produced by await_user_input (AwaitingUser state)"
                );
                return Ok(LoopDirective::AwaitUser);
            }
            DrainOutcome::EmptyAndSealed { .. } => {
                if run.status() == RunStatus::AwaitingUser {
                    // #1272: No user input pending; stay awaiting without
                    // advancing epoch — the buffer was sealed by the legacy
                    // path but this code path is still reachable from
                    // adapters whose await_user_input falls back to drain_input.
                    return Ok(LoopDirective::AwaitUser);
                }
                // #1272: advance epoch before apply_drain_decision.
                run.advance_drain_epoch();
                #[allow(unused_assignments)]
                {
                    expected_epoch = expected_epoch.next();
                }

                // #1272: terminal claim exactly once per run, at the seal point.
                if !port.claim_terminal(run.id()) {
                    cancel_run(run, port).await?;
                    return Ok(LoopDirective::Terminal);
                }
                let text = terminal_text.as_deref();
                run.apply_drain_decision(DrainDecision::EmptyAndSealed, text)?;
                emit_events(run, port).await?;
                return Ok(LoopDirective::Terminal);
            }
        }
    }
}

/// Execute one step: freeze input → build context → compact → invoke model →
/// handle response. Updates `terminal_text` with the last assistant text.
async fn execute_step<P>(
    run: &mut Run,
    cancel: &CancellationToken,
    port: &mut P,
    guard: &mut StuckGuard,
    inputs: &[LoopInput],
    terminal_text: &mut Option<String>,
) -> Result<(), LoopEngineError>
where
    P: RunLoopPort,
{
    let step_id = sdk::RunStepId::new_v7();
    let step_cancel = cancel.child_token();
    port.register_step_scope(run.id(), step_id.clone(), step_cancel.clone());
    port.freeze_step(&step_id, inputs);
    if let Err(error) = port.accept_step_input(&step_id).await {
        fail_run(run, port, error.to_string()).await?;
        return Ok(());
    }
    let step_id = run.begin_step_with_id(step_id)?;
    emit_events(run, port).await?;
    // -- compaction check --
    let needs_compaction =
        match await_interruptible(run, &step_cancel, port.needs_compaction()).await {
            Interrupt::Completed(result) => result?,
            Interrupt::Cancelled => {
                handle_step_control(run, port).await?;
                return Ok(());
            }
            Interrupt::TimedOut => {
                timeout_run(run, port).await?;
                return Ok(());
            }
        };
    if needs_compaction {
        run.transition(RunTransition::BeginCompaction)?;
        match await_interruptible(run, &step_cancel, port.compact(&step_cancel)).await {
            Interrupt::Completed(Ok(())) => {}
            Interrupt::Completed(Err(LoopEngineError::Cancelled)) | Interrupt::Cancelled => {
                return handle_step_control(run, port).await;
            }
            Interrupt::Completed(Err(error)) => return Err(error),
            Interrupt::TimedOut => {
                timeout_run(run, port).await?;
                return Ok(());
            }
        }
        run.transition(RunTransition::CompactionCompleted)?;
    }

    if handle_interrupt(run, cancel, port).await? {
        return Ok(());
    }
    run.transition(RunTransition::ContextPrepared)?;
    let mut compacted_after_context_too_long = false;
    let (model_step, token_usage) = loop {
        match await_interruptible(run, &step_cancel, port.invoke_model(&step_cancel)).await {
            Interrupt::Completed(Ok(result)) => break result,
            Interrupt::Completed(Err(LoopEngineError::Cancelled)) | Interrupt::Cancelled => {
                handle_step_control(run, port).await?;
                return Ok(());
            }
            Interrupt::Completed(Err(LoopEngineError::NeedsCompaction(error))) => {
                if compacted_after_context_too_long {
                    fail_run(
                        run,
                        port,
                        format!("compact 后 Provider 仍报告 context 超限：{error}"),
                    )
                    .await?;
                    return Ok(());
                }
                run.transition(RunTransition::ModelContextExceeded)?;
                match await_interruptible(run, &step_cancel, port.compact(&step_cancel)).await {
                    Interrupt::Completed(Ok(())) => {}
                    Interrupt::Completed(Err(LoopEngineError::Cancelled))
                    | Interrupt::Cancelled => {
                        handle_step_control(run, port).await?;
                        return Ok(());
                    }
                    Interrupt::Completed(Err(error)) => return Err(error),
                    Interrupt::TimedOut => {
                        timeout_run(run, port).await?;
                        return Ok(());
                    }
                }
                run.transition(RunTransition::CompactionCompleted)?;
                run.transition(RunTransition::ContextPrepared)?;
                compacted_after_context_too_long = true;
            }
            Interrupt::Completed(Err(error)) => {
                fail_run(run, port, error.to_string()).await?;
                return Ok(());
            }
            Interrupt::TimedOut => {
                timeout_run(run, port).await?;
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
    if handle_interrupt(run, cancel, port).await? {
        return Ok(());
    }
    run.record_model_invocation(&step_id, model_invocation(&model_step))?;
    run.transition(RunTransition::ModelInvoked)?;
    log::debug!(
        target: crate::LOG_TARGET,
        "[run_loop] model_step={} run_id={}",
        model_step_label(&model_step),
        short(run.id()),
    );

    // #1272: track the last assistant text for terminal claim
    let assistant_text = model_step_text(&model_step);
    if assistant_text.trim().is_empty() {
        log::warn!(
            target: crate::LOG_TARGET,
            "{}",
            serde_json::json!({
                "event_type": "empty_terminal_text",
                "model_step": model_step_label(&model_step),
                "step_id": step_id.to_string(),
            })
        );
    }
    *terminal_text = Some(assistant_text);

    match model_step {
        ModelStep::Complete { text: _ } => {
            match guard.inspect_text(terminal_text.as_deref().unwrap_or("")) {
                decision @ StuckDecision::SoftBlock { .. } => {
                    record_stuck(run, port, &decision).await?;
                    run.transition(RunTransition::ContinueAfterResponse)?;
                    run.complete_step(&step_id)?;
                    port.finalize_step(&step_id).await?;
                    return Ok(());
                }
                decision @ StuckDecision::HardPause { .. } => {
                    let reason = match &decision {
                        StuckDecision::HardPause { reason } => reason.clone(),
                        _ => unreachable!(),
                    };
                    record_stuck(run, port, &decision).await?;
                    fail_run(run, port, reason).await?;
                    return Ok(());
                }
                StuckDecision::Allow | StuckDecision::Fail { .. } => {}
            }
            // #1272: Complete goes to DrainingInput (not Finishing→Finish)
            run.transition(RunTransition::ContinueAfterResponse)?;
            run.complete_step(&step_id)?;
            port.finalize_step(&step_id).await?;
            // Loop back to drain — adapter returns EmptyAndSealed for Complete
        }
        ModelStep::Continue { text: _ } => {
            let decision = guard.inspect_text(terminal_text.as_deref().unwrap_or(""));
            match decision {
                StuckDecision::SoftBlock { .. } => record_stuck(run, port, &decision).await?,
                StuckDecision::HardPause { ref reason } => {
                    let reason = reason.clone();
                    record_stuck(run, port, &decision).await?;
                    fail_run(run, port, reason).await?;
                    return Ok(());
                }
                StuckDecision::Allow | StuckDecision::Fail { .. } => {}
            }
            // #1272: Continue goes to DrainingInput
            run.transition(RunTransition::ContinueAfterResponse)?;
            run.complete_step(&step_id)?;
            port.finalize_step(&step_id).await?;
        }
        ModelStep::StopHookBlocked { text: _ } => {
            let text_decision = guard.inspect_text(terminal_text.as_deref().unwrap_or(""));
            record_stuck(run, port, &text_decision).await?;
            let decision = guard.record_stop_hook_block();
            record_stuck(run, port, &decision).await?;
            match decision {
                StuckDecision::Fail { reason } => {
                    fail_run(run, port, reason).await?;
                    return Ok(());
                }
                StuckDecision::Allow
                | StuckDecision::SoftBlock { .. }
                | StuckDecision::HardPause { .. } => {
                    // #1272: StopHookBlocked goes to DrainingInput
                    run.transition(RunTransition::ContinueAfterResponse)?;
                    run.complete_step(&step_id)?;
                    port.finalize_step(&step_id).await?;
                }
            }
        }
        ModelStep::Tools { text: _, calls } => {
            if let decision @ StuckDecision::SoftBlock { .. } =
                guard.inspect_text(terminal_text.as_deref().unwrap_or(""))
            {
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
                        return Ok(());
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
                &step_cancel,
                port.execute_tools(run.id(), &step_id, &guarded_calls, &step_cancel),
            )
            .await
            {
                Interrupt::Completed(Ok(step)) => step,
                Interrupt::Completed(Err(LoopEngineError::Cancelled)) | Interrupt::Cancelled => {
                    handle_step_control(run, port).await?;
                    return Ok(());
                }
                Interrupt::Completed(Err(error)) => {
                    fail_run(run, port, error.to_string()).await?;
                    return Ok(());
                }
                Interrupt::TimedOut => {
                    timeout_run(run, port).await?;
                    return Ok(());
                }
            };
            if handle_interrupt(run, cancel, port).await? {
                return Ok(());
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
                    // #1272: ToolsCompleted → DrainingInput (not PreparingContext)
                    run.transition(RunTransition::ToolsCompleted)?;
                }
                ToolStep::AwaitUser => {
                    run.complete_step(&step_id)?;
                    run.transition(RunTransition::AwaitUser)?;
                    emit_events(run, port).await?;
                    // Return to caller; the caller will call run_loop again
                    // with drain_input picking up the user response.
                    return Ok(());
                }
            }
        }
    }
    Ok(())
}

/// Extract assistant text from a model step for terminal tracking.
fn model_step_text(step: &ModelStep) -> String {
    match step {
        ModelStep::Complete { text }
        | ModelStep::Continue { text }
        | ModelStep::StopHookBlocked { text }
        | ModelStep::Tools { text, .. } => text.clone(),
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

enum ControlDirective {
    Continue,
    Terminal,
}

async fn handle_pending_control<P>(
    run: &mut Run,
    port: &mut P,
) -> Result<Option<ControlDirective>, LoopEngineError>
where
    P: RunLoopPort,
{
    let Some(control) = port.take_control(run.id()) else {
        return Ok(None);
    };
    let active_step = run.active_step_id();
    match control {
        crate::domain::agent_run::RunControl::CancelStep { step_id, .. } => {
            if active_step.as_ref() != Some(&step_id) {
                return Err(LoopEngineError::Adapter(
                    "CancelRunStep 与当前 Step identity 不匹配".to_string(),
                ));
            }
            finish_cancelled_step(run, port, &step_id).await?;
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
            emit_events(run, port).await?;
            if let Some(step_id) = active_step {
                port.finalize_cancelled_step(&step_id).await?;
            }
            run.finish_termination()?;
            emit_events(run, port).await?;
            Ok(Some(ControlDirective::Terminal))
        }
    }
}

async fn finish_cancelled_step<P>(
    run: &mut Run,
    port: &mut P,
    step_id: &sdk::RunStepId,
) -> Result<(), LoopEngineError>
where
    P: RunLoopPort,
{
    match run.request_step_cancellation(step_id) {
        crate::domain::agent_run::RunStepCancellationRequest::Accepted => {}
        crate::domain::agent_run::RunStepCancellationRequest::AlreadyCancelling => return Ok(()),
        outcome => {
            return Err(LoopEngineError::Adapter(format!(
                "取消当前 Step 时获得了非预期结果：{outcome:?}"
            )));
        }
    }
    emit_events(run, port).await?;
    run.begin_step_finalization(step_id)?;
    emit_events(run, port).await?;
    port.finalize_cancelled_step(step_id).await?;
    run.finish_cancelled_step(step_id)?;
    emit_events(run, port).await
}

async fn handle_step_control<P>(run: &mut Run, port: &mut P) -> Result<(), LoopEngineError>
where
    P: RunLoopPort,
{
    match handle_pending_control(run, port).await? {
        Some(ControlDirective::Continue) => Ok(()),
        Some(ControlDirective::Terminal) => Ok(()),
        None => {
            cancel_run(run, port).await?;
            Ok(())
        }
    }
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
    // #1272: if the run is already terminal (e.g. Failed after a
    // timeout inside execute_step), return immediately without
    // attempting another timeout transition.
    if run.status().is_terminal() {
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
