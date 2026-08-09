use std::future::Future;
use std::time::Instant;

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

use crate::application::activity::{ActivityError, ActivityTerminal};
use crate::application::hook::stop_coordination::StopHookDecision;
use crate::application::loop_engine::RunLoop;
use crate::application::run::context::RuntimeContext;
use crate::application::run::execution_state::RunExecutionState;
use crate::application::tool::agent::ToolCall;
use crate::domain::agent_run::{
    DrainDecision, InteractionContinuation, ModelInvocation, Run, RunStatus, RunTransition,
    RunTransitionError, RuntimeLifecycleEvent, StopHookBlockResult, ToolCallStatus,
};

use super::{StuckDecision, StuckGuard};

mod contracts;
mod control_driver;
mod interaction_driver;
mod phases;
mod step_driver;

pub use contracts::*;
pub(crate) use control_driver::fail_run;
use control_driver::*;
use interaction_driver::*;
use phases::*;
use step_driver::*;

pub async fn execute_prepared_loop(
    run: &mut Run,
    execution: &mut RunExecutionState,
    context: &RuntimeContext,
    activities: std::sync::Arc<crate::application::activity::ActivityCoordinator>,
    cancel: &CancellationToken,
    loop_context: &mut RunLoop<'_>,
) -> Result<LoopDirective, LoopEngineError> {
    loop_context.bind_activity_context(
        activities.clone(),
        context.provider_ref().model.model.clone(),
    );
    activities.publish_snapshot();
    let result = run_loop(run, execution, cancel, loop_context).await;

    let _event_sink = context.event_sink();
    result
}

pub async fn run_loop(
    run: &mut Run,
    execution: &mut RunExecutionState,
    cancel: &CancellationToken,
    port: &mut RunLoop<'_>,
) -> Result<LoopDirective, LoopEngineError> {
    #[cfg(test)]
    let uses_ephemeral_test_activities = port.activities_are_unbound();
    #[cfg(test)]
    port.ensure_test_activities(run.id());
    let directive = run_loop_body(run, execution, cancel, port).await;
    #[cfg(test)]
    if uses_ephemeral_test_activities {
        port.clear_test_activities();
    }
    directive
}

async fn run_loop_body(
    run: &mut Run,
    execution: &mut RunExecutionState,
    cancel: &CancellationToken,
    port: &mut RunLoop<'_>,
) -> Result<LoopDirective, LoopEngineError> {
    if run.status() == RunStatus::Created {
        run.start_draining()?;
        emit_events(run, execution, port).await?;
    }

    log::debug!(
        target: crate::LOG_TARGET,
        "[run_loop] entered run_id={} parent={} spec={:?}",
        run.id(),
        run.parent_id().map(|id| id.to_string()).unwrap_or_else(|| "none".into()),
        run.spec(),
    );

    let mut guard = StuckGuard::new();
    // #1272: engine-owned epoch for per-run drain linearization.
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
        if let Some(control) = handle_pending_control(run, execution, port).await? {
            if matches!(control, ControlDirective::Terminal) {
                return Ok(LoopDirective::Terminal);
            }
            continue;
        }
        if handle_interrupt(run, execution, cancel, port).await? {
            return Ok(LoopDirective::Terminal);
        }
        // #1272: failed/cancelled runs are terminal; do not drain again.
        if run.status().is_terminal() {
            return Ok(LoopDirective::Terminal);
        }

        // AwaitingUser with an active Interaction has two legitimate ingress
        // sources. Wait for both in one select so either source can wake the
        // Run; never park exclusively on the Session input mailbox.
        let awaiting_user = run.status() == RunStatus::AwaitingUser;
        let outcome = if awaiting_user && run.pending_interaction().is_some() {
            let pending = run
                .pending_interaction()
                .expect("checked pending interaction above");
            log::debug!(
                target: crate::LOG_TARGET,
                "[run_loop] awaiting unified ingress run_id={} request_id={} continuation={:?}",
                run.id(),
                pending.request_id,
                pending.continuation,
            );
            match await_run_ingress(run, execution, expected_epoch, cancel, port.input_mut())
                .await?
            {
                AwaitingRunIngress::Interaction(completion) => {
                    log::debug!(
                        target: crate::LOG_TARGET,
                        "[run_loop] interaction completion observed run_id={} request_id={} resolution={}",
                        run.id(),
                        completion.metadata().request_id,
                        interaction_resolution_kind(&completion),
                    );
                    handle_interaction_completion(run, execution, port, cancel, completion).await?;
                    continue;
                }
                AwaitingRunIngress::Input(input_outcome) => match input_outcome {
                    InputDrainOutcome::Drained(outcome) => {
                        log::debug!(
                            target: crate::LOG_TARGET,
                            "[run_loop] input drain completed run_id={} status={:?} pending_interaction={} outcome={}",
                            run.id(),
                            run.status(),
                            run.pending_interaction().is_some(),
                            drain_outcome_kind(&outcome),
                        );
                        outcome
                    }
                    InputDrainOutcome::Cancelled => {
                        if let Some(control) = handle_pending_control(run, execution, port).await? {
                            return Ok(match control {
                                ControlDirective::Continue => LoopDirective::AwaitUser,
                                ControlDirective::Terminal => LoopDirective::Terminal,
                            });
                        }
                        terminate_interrupted_run(run, execution, port).await?;
                        return Ok(LoopDirective::Terminal);
                    }
                    InputDrainOutcome::TimedOut => {
                        timeout_run(run, execution, port).await?;
                        return Ok(LoopDirective::Terminal);
                    }
                },
            }
        } else {
            log::debug!(
                target: crate::LOG_TARGET,
                "[run_loop] input drain starting run_id={} status={:?} awaiting_user={} pending_interaction={} epoch={:?}",
                run.id(),
                run.status(),
                awaiting_user,
                run.pending_interaction().is_some(),
                expected_epoch,
            );
            match run_input_drain_phase(
                run,
                awaiting_user,
                expected_epoch,
                cancel,
                port.input_mut(),
            )
            .await?
            {
                InputDrainOutcome::Drained(outcome) => {
                    log::debug!(
                        target: crate::LOG_TARGET,
                        "[run_loop] input drain completed run_id={} status={:?} pending_interaction={} outcome={}",
                        run.id(),
                        run.status(),
                        run.pending_interaction().is_some(),
                        drain_outcome_kind(&outcome),
                    );
                    outcome
                }
                InputDrainOutcome::Cancelled => {
                    log::debug!(
                        target: crate::LOG_TARGET,
                        "[run_loop] input drain interrupted run_id={} result=cancelled status={:?} pending_interaction={}",
                        run.id(),
                        run.status(),
                        run.pending_interaction().is_some(),
                    );
                    if let Some(control) = handle_pending_control(run, execution, port).await? {
                        return Ok(match control {
                            ControlDirective::Continue => LoopDirective::AwaitUser,
                            ControlDirective::Terminal => LoopDirective::Terminal,
                        });
                    }
                    terminate_interrupted_run(run, execution, port).await?;
                    return Ok(LoopDirective::Terminal);
                }
                InputDrainOutcome::TimedOut => {
                    log::debug!(
                        target: crate::LOG_TARGET,
                        "[run_loop] input drain interrupted run_id={} result=timed_out status={:?} pending_interaction={}",
                        run.id(),
                        run.status(),
                        run.pending_interaction().is_some(),
                    );
                    timeout_run(run, execution, port).await?;
                    return Ok(LoopDirective::Terminal);
                }
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
                    transition_and_emit(run, execution, port, RunTransition::UserResumed).await?;
                }
                // batch is non-empty per DrainOutcome::Ready contract
                run.apply_drain_decision(DrainDecision::Inputs, None)?;
                execute_step(
                    run,
                    execution,
                    cancel,
                    port,
                    &mut guard,
                    &batch,
                    &mut terminal_text,
                )
                .await?;
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
                    transition_and_emit(run, execution, port, RunTransition::UserResumed).await?;
                }
                run.apply_drain_decision(DrainDecision::InternalContinuation, None)?;
                execute_step(
                    run,
                    execution,
                    cancel,
                    port,
                    &mut guard,
                    &batch,
                    &mut terminal_text,
                )
                .await?;
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

                let text = terminal_text.as_deref();
                run.apply_drain_decision(DrainDecision::EmptyAndSealed, text)?;
                emit_events(run, execution, port).await?;
                return Ok(LoopDirective::Terminal);
            }
        }
    }
}
