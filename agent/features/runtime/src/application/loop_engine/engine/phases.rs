use super::*;

pub(super) enum InputDrainOutcome {
    Drained(DrainOutcome),
    Cancelled,
    TimedOut,
}

pub(super) enum AwaitingRunIngress {
    Input(InputDrainOutcome),
    Interaction(crate::application::interaction::port::InteractionResolution),
}

pub(super) fn drain_outcome_kind(outcome: &DrainOutcome) -> &'static str {
    match outcome {
        DrainOutcome::Ready { .. } => "ready",
        DrainOutcome::InternalContinuation { .. } => "internal_continuation",
        DrainOutcome::EmptyAndSealed { .. } => "empty_and_sealed",
        DrainOutcome::NoInput { .. } => "no_input",
    }
}

pub(super) fn interaction_resolution_kind(
    resolution: &crate::application::interaction::port::InteractionResolution,
) -> &'static str {
    use crate::application::interaction::port::{InteractionCompletion, InteractionResolution};

    match resolution {
        InteractionResolution::Resolved {
            completion: InteractionCompletion::Replied(_),
            ..
        } => "replied",
        InteractionResolution::Resolved {
            completion: InteractionCompletion::Cancelled(_),
            ..
        } => "cancelled",
        InteractionResolution::Closed { .. } => "closed",
    }
}

pub(super) async fn run_input_drain_phase<P>(
    run: &Run,
    awaiting_user: bool,
    expected_epoch: DrainEpoch,
    cancel: &CancellationToken,
    input: &mut P,
) -> Result<InputDrainOutcome, LoopEngineError>
where
    P: InputPort + ?Sized,
{
    let drain_future = if awaiting_user {
        input.await_user_input(expected_epoch)
    } else {
        input.drain_input(expected_epoch)
    };
    match await_interruptible(run, cancel, drain_future).await {
        Interrupt::Completed(result) => result.map(InputDrainOutcome::Drained),
        Interrupt::Cancelled => Ok(InputDrainOutcome::Cancelled),
        Interrupt::TimedOut => Ok(InputDrainOutcome::TimedOut),
    }
}

pub(super) async fn await_run_ingress<P>(
    run: &Run,
    execution: &mut RunExecutionState,
    expected_epoch: DrainEpoch,
    cancel: &CancellationToken,
    input: &mut P,
) -> Result<AwaitingRunIngress, LoopEngineError>
where
    P: InputPort + ?Sized,
{
    let Some(mut active_interaction) = execution.take_active_interaction() else {
        return Err(LoopEngineError::Adapter(
            "Run 处于 AwaitingUser 且声明 pending interaction，但 interaction mailbox 为空"
                .to_string(),
        ));
    };
    let run_id = active_interaction.metadata.run_id.clone();
    let request_id = active_interaction.metadata.request_id.clone();
    let input_future = run_input_drain_phase(run, true, expected_epoch, cancel, input);
    tokio::pin!(input_future);

    tokio::select! {
        biased;
        completion = &mut active_interaction.receiver => {
            let resolution = match completion {
                Ok(completion) => {
                    crate::application::interaction::port::InteractionResolution::Resolved {
                        metadata: active_interaction.metadata,
                        completion,
                    }
                }
                Err(_) => crate::application::interaction::port::InteractionResolution::Closed {
                    metadata: active_interaction.metadata,
                },
            };
            log::debug!(
                target: crate::LOG_TARGET,
                "[run_loop] unified ingress woke by interaction run_id={} request_id={} resolution={}",
                run_id,
                request_id,
                interaction_resolution_kind(&resolution),
            );
            Ok(AwaitingRunIngress::Interaction(resolution))
        }
        input_outcome = &mut input_future => {
            let input_outcome = input_outcome?;
            log::debug!(
                target: crate::LOG_TARGET,
                "[run_loop] unified ingress woke by session input run_id={} request_id={}",
                run_id,
                request_id,
            );
            crate::application::interaction::coordinator::InteractionCoordinator::store_mailbox_receiver(
                execution,
                active_interaction.metadata,
                active_interaction.receiver,
            )
            .map_err(|error| {
                LoopEngineError::Adapter(format!(
                    "interaction mailbox restore failed: {error:?}"
                ))
            })?;
            Ok(AwaitingRunIngress::Input(input_outcome))
        }
    }
}

pub(super) enum StepInputOutcome {
    Accepted(sdk::RunStepId),
    Rejected(LoopEngineError),
}

pub(super) async fn run_step_input_phase<P>(
    run: &mut Run,
    execution: &mut RunExecutionState,
    step_id: sdk::RunStepId,
    inputs: &[LoopInput],
    persistence: &mut P,
) -> Result<StepInputOutcome, LoopEngineError>
where
    P: StepPersistencePort + ?Sized,
{
    execution.begin_step();
    freeze_step(execution, persistence, run.id(), &step_id, inputs);
    match persistence.accept_step_input(execution, &step_id).await {
        Ok(()) => Ok(StepInputOutcome::Accepted(run.begin_step_with_id(step_id)?)),
        Err(error) => Ok(StepInputOutcome::Rejected(error)),
    }
}

pub(super) enum StepFinalizationOutcome {
    Committed,
}

pub(super) async fn run_step_finalization_phase<P>(
    execution: &mut RunExecutionState,
    persistence: &mut P,
    step_id: &sdk::RunStepId,
    cause: crate::ports::FinalizeCause,
) -> Result<StepFinalizationOutcome, LoopEngineError>
where
    P: StepPersistencePort + ?Sized,
{
    finalize_step(execution, persistence, step_id, cause).await?;
    Ok(StepFinalizationOutcome::Committed)
}

#[derive(Debug)]
pub(super) enum ContextCompactionOutcome {
    Ready,
    Cancelled,
    TimedOut,
}

pub(super) async fn run_context_compaction_phase<P>(
    run: &Run,
    execution: &mut RunExecutionState,
    cancel: &CancellationToken,
    compaction: &mut P,
    progress: std::sync::Arc<dyn CompactProgressView>,
) -> Result<ContextCompactionOutcome, LoopEngineError>
where
    P: CompactionPort + ?Sized,
{
    match await_interruptible(run, cancel, compaction.compact(execution, cancel, progress)).await {
        Interrupt::Completed(Ok(())) => Ok(ContextCompactionOutcome::Ready),
        Interrupt::Completed(Err(LoopEngineError::Cancelled)) | Interrupt::Cancelled => {
            Ok(ContextCompactionOutcome::Cancelled)
        }
        Interrupt::Completed(Err(error)) => Err(error),
        Interrupt::TimedOut => Ok(ContextCompactionOutcome::TimedOut),
    }
}

pub(super) enum ModelInvocationOutcome {
    Invoked(ModelStep, StepTokenUsage),
    NeedsCompaction(String),
    Failed(LoopEngineError),
    Cancelled,
    TimedOut,
}

pub(super) async fn run_model_invocation_phase<P>(
    run: &Run,
    execution: &mut RunExecutionState,
    step_id: &sdk::RunStepId,
    cancel: &CancellationToken,
    model: &mut P,
) -> ModelInvocationOutcome
where
    P: ModelInvocationPort + ?Sized,
{
    let invocation = model.invoke_model(execution, step_id, cancel);
    let result = if let Some(remaining) = run.remaining_time(Instant::now()) {
        if remaining.is_zero() {
            return ModelInvocationOutcome::TimedOut;
        }
        match tokio::time::timeout(remaining, invocation).await {
            Ok(result) => result,
            Err(_) => return ModelInvocationOutcome::TimedOut,
        }
    } else {
        invocation.await
    };
    match result {
        Ok((step, usage)) => ModelInvocationOutcome::Invoked(step, usage),
        Err(LoopEngineError::NeedsCompaction(error)) => {
            ModelInvocationOutcome::NeedsCompaction(error)
        }
        Err(LoopEngineError::Cancelled) => ModelInvocationOutcome::Cancelled,
        Err(error) => ModelInvocationOutcome::Failed(error),
    }
}

#[derive(Debug)]
pub(super) enum ToolRoundPhaseOutcome {
    Completed(crate::application::tool::coordination::ToolRoundOutcome),
    Failed(LoopEngineError),
    Cancelled,
    TimedOut,
}

pub(super) async fn run_tool_round_phase<P>(
    run: &Run,
    execution: &mut RunExecutionState,
    run_id: &sdk::RunId,
    step_id: &sdk::RunStepId,
    calls: &[(ToolCall, ToolGuardDecision)],
    cancel: &CancellationToken,
    tools: &mut P,
) -> ToolRoundPhaseOutcome
where
    P: ToolOrchestrationPort + ?Sized,
{
    let tool_execution = tools.execute_tools(execution, run_id, step_id, calls, cancel);
    if let Some(remaining) = run.remaining_time(Instant::now()) {
        match tokio::time::timeout(remaining, tool_execution).await {
            Ok(Ok(outcome)) => ToolRoundPhaseOutcome::Completed(outcome),
            Ok(Err(LoopEngineError::Cancelled)) => ToolRoundPhaseOutcome::Cancelled,
            Ok(Err(error)) => ToolRoundPhaseOutcome::Failed(error),
            Err(_) => ToolRoundPhaseOutcome::TimedOut,
        }
    } else {
        match tool_execution.await {
            Ok(outcome) => ToolRoundPhaseOutcome::Completed(outcome),
            Err(LoopEngineError::Cancelled) => ToolRoundPhaseOutcome::Cancelled,
            Err(error) => ToolRoundPhaseOutcome::Failed(error),
        }
    }
}

pub(super) enum Interrupt<T> {
    Completed(T),
    Cancelled,
    TimedOut,
}

pub(super) async fn await_interruptible<F, T>(
    run: &Run,
    cancel: &CancellationToken,
    future: F,
) -> Interrupt<T>
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
