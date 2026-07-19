use std::time::{Duration, Instant};

use crate::domain::agent_run::ToolCall;

use super::event::{RunDomainEvent, RunId};
use super::spec::RunSpec;
use super::state::{
    DrainDecision, InteractionContinuation, PendingInteraction, RunCancellationRequest, RunStatus,
    RunStep, RunStepCancellationRequest, RunStepId, RunStepStatus, RunTerminationRequest,
    RunTransition, RunTransitionError, RunTransitionReason,
};
use super::step::{ModelInvocation, RunToolCall, ToolCallStatus};

#[derive(Debug)]
pub struct Run {
    id: RunId,
    spec: RunSpec,
    parent_id: Option<RunId>,
    status: RunStatus,
    termination: Option<(sdk::RunTerminationReason, sdk::ControlDeadline)>,
    pending_interaction: Option<PendingInteraction>,
    steps: Vec<RunStep>,
    started_at: Option<Instant>,
    events: Vec<RunDomainEvent>,
}

impl Run {
    pub fn new(spec: RunSpec, parent_id: Option<RunId>) -> Self {
        Self::with_id(RunId::new_v7(), spec, parent_id)
    }

    pub fn with_id(id: RunId, spec: RunSpec, parent_id: Option<RunId>) -> Self {
        Self {
            id,
            spec,
            parent_id,
            status: RunStatus::Created,
            termination: None,
            pending_interaction: None,
            steps: Vec::new(),
            started_at: None,
            events: Vec::new(),
        }
    }

    pub fn id(&self) -> &RunId {
        &self.id
    }

    pub fn parent_id(&self) -> Option<&RunId> {
        self.parent_id.as_ref()
    }

    pub fn spec(&self) -> &RunSpec {
        &self.spec
    }

    pub fn status(&self) -> RunStatus {
        self.status
    }

    pub fn steps(&self) -> &[RunStep] {
        &self.steps
    }

    pub fn pending_interaction(&self) -> Option<&PendingInteraction> {
        self.pending_interaction.as_ref()
    }

    pub fn begin_interaction(
        &mut self,
        request_id: sdk::InteractionRequestId,
        continuation: InteractionContinuation,
    ) -> Result<(), RunTransitionError> {
        if self.rejects_controlled_work() {
            return Err(RunTransitionError::RunNotActive(self.status));
        }
        if let Some(pending) = &self.pending_interaction {
            return Err(RunTransitionError::InteractionAlreadyPending(
                pending.request_id.clone(),
            ));
        }
        let allowed = matches!(
            (&continuation, self.status),
            (
                InteractionContinuation::CompleteToolCall(_)
                    | InteractionContinuation::ContinueAfterHardPause,
                RunStatus::ExecutingTools
            ) | (
                InteractionContinuation::ContinueToolApproval(_),
                RunStatus::AwaitingToolApproval
            ) | (
                InteractionContinuation::ContinuePlanApproval,
                RunStatus::ApplyingResponse
            )
        );
        if !allowed {
            return Err(RunTransitionError::RunNotActive(self.status));
        }
        self.pending_interaction = Some(PendingInteraction {
            request_id: request_id.clone(),
            continuation,
        });
        self.apply_state_transition(RunStatus::AwaitingUser, RunTransitionReason::AwaitUser);
        self.events.push(RunDomainEvent::AwaitingUser {
            run_id: self.id.clone(),
            parent_run_id: self.parent_id.clone(),
            request_id,
        });
        Ok(())
    }

    pub fn complete_interaction(
        &mut self,
        request_id: &sdk::InteractionRequestId,
    ) -> Result<InteractionContinuation, RunTransitionError> {
        let pending = self
            .pending_interaction
            .as_ref()
            .ok_or(RunTransitionError::NoPendingInteraction)?;
        if &pending.request_id != request_id {
            return Err(RunTransitionError::InteractionRequestMismatch {
                expected: pending.request_id.clone(),
                received: request_id.clone(),
            });
        }
        let pending = self.pending_interaction.take().expect("checked above");
        self.apply_state_transition(
            pending.continuation.resume_status(),
            RunTransitionReason::UserResumed,
        );
        self.events.push(RunDomainEvent::Resumed {
            run_id: self.id.clone(),
            parent_run_id: self.parent_id.clone(),
            request_id: request_id.clone(),
        });
        Ok(pending.continuation)
    }

    pub fn cancel_interaction(
        &mut self,
        request_id: &sdk::InteractionRequestId,
    ) -> Result<InteractionContinuation, RunTransitionError> {
        let pending = self
            .pending_interaction
            .as_ref()
            .ok_or(RunTransitionError::NoPendingInteraction)?;
        if &pending.request_id != request_id {
            return Err(RunTransitionError::InteractionRequestMismatch {
                expected: pending.request_id.clone(),
                received: request_id.clone(),
            });
        }
        Ok(self
            .pending_interaction
            .take()
            .expect("checked above")
            .continuation)
    }

    pub fn events(&self) -> &[RunDomainEvent] {
        &self.events
    }

    pub fn drain_events(&mut self) -> Vec<RunDomainEvent> {
        std::mem::take(&mut self.events)
    }

    pub fn restore_events(&mut self, mut events: Vec<RunDomainEvent>) {
        events.append(&mut self.events);
        self.events = events;
    }

    pub fn is_terminal(&self) -> bool {
        self.status.is_terminal()
    }

    pub fn started_at(&self) -> Option<Instant> {
        self.started_at
    }

    pub fn remaining_time(&self, now: Instant) -> Option<Duration> {
        if self.spec.timeout.is_zero() {
            return None;
        }
        let started_at = self.started_at?;
        Some(
            self.spec
                .timeout
                .saturating_sub(now.duration_since(started_at)),
        )
    }

    pub fn has_timed_out(&self, now: Instant) -> bool {
        !self.spec.timeout.is_zero()
            && self
                .started_at
                .is_some_and(|started_at| now.duration_since(started_at) >= self.spec.timeout)
    }

    pub fn transition(
        &mut self,
        transition: RunTransition,
    ) -> Result<RunStatus, RunTransitionError> {
        if self.status == RunStatus::InvokingModel && transition == RunTransition::ModelInvoked {
            let Some(step) = self.steps.iter().find(|step| step.is_active()) else {
                return Err(RunTransitionError::StepIncomplete);
            };
            if step.invocation().is_none() {
                return Err(RunTransitionError::StepIncomplete);
            }
        }
        let next = match (self.status, transition) {
            (RunStatus::Created, RunTransition::Start) => RunStatus::PreparingContext,
            (RunStatus::Created, RunTransition::StartDraining) => RunStatus::DrainingInput,
            (RunStatus::DrainingInput, RunTransition::DrainInputs)
            | (RunStatus::DrainingInput, RunTransition::DrainInternalContinuation) => {
                RunStatus::PreparingContext
            }
            (RunStatus::DrainingInput, RunTransition::DrainEmptyAndSealed) => RunStatus::Completed,
            (RunStatus::PreparingContext, RunTransition::BeginCompaction) => RunStatus::Compacting,
            (RunStatus::Compacting, RunTransition::CompactionCompleted) => {
                RunStatus::PreparingContext
            }
            (RunStatus::PreparingContext, RunTransition::ContextPrepared) => {
                RunStatus::InvokingModel
            }
            (RunStatus::InvokingModel, RunTransition::RetryModel) => RunStatus::InvokingModel,
            (RunStatus::InvokingModel, RunTransition::ModelContextExceeded) => {
                RunStatus::Compacting
            }
            (RunStatus::InvokingModel, RunTransition::ModelInvoked) => RunStatus::ApplyingResponse,
            (RunStatus::ApplyingResponse, RunTransition::ResponseWithTools) => {
                RunStatus::AwaitingToolApproval
            }
            (RunStatus::ApplyingResponse, RunTransition::ResponseWithoutTools) => {
                RunStatus::Finishing
            }
            (RunStatus::ApplyingResponse, RunTransition::ContinueAfterResponse) => {
                RunStatus::PreparingContext
            }
            (RunStatus::AwaitingToolApproval, RunTransition::ToolsApproved) => {
                RunStatus::ExecutingTools
            }
            (RunStatus::AwaitingToolApproval, RunTransition::AwaitUser)
            | (RunStatus::ExecutingTools, RunTransition::AwaitUser) => RunStatus::AwaitingUser,
            (RunStatus::AwaitingUser, RunTransition::UserResumed)
            | (RunStatus::ExecutingTools, RunTransition::ToolsCompleted) => {
                RunStatus::PreparingContext
            }
            (RunStatus::Finishing, RunTransition::Finish) => RunStatus::Completed,
            (RunStatus::FinalizingStep, RunTransition::StepCancelled) => RunStatus::DrainingInput,
            (RunStatus::Terminating, RunTransition::TerminationFinished) => RunStatus::Terminated,
            (RunStatus::Cancelling, RunTransition::CancellationFinished) => RunStatus::Cancelled,
            (from, transition) => {
                log::warn!(
                    target: crate::LOG_TARGET,
                    "run state transition rejected: run_id={} parent_run_id={} from={from:?} requested_transition={transition:?} reason={:?}",
                    self.id,
                    self.parent_id_display(),
                    RunTransitionReason::from(transition),
                );
                return Err(RunTransitionError::IllegalTransition { from, transition });
            }
        };

        self.apply_state_transition(next, RunTransitionReason::from(transition));
        if transition == RunTransition::Start {
            self.started_at = Some(Instant::now());
            self.events.push(RunDomainEvent::Started {
                run_id: self.id.clone(),
                parent_run_id: self.parent_id.clone(),
            });
        }
        Ok(next)
    }

    fn parent_id_display(&self) -> String {
        self.parent_id
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_else(|| "-".to_string())
    }

    fn apply_state_transition(&mut self, to: RunStatus, reason: RunTransitionReason) {
        let from = self.status;
        self.status = to;
        self.events.push(RunDomainEvent::Transitioned {
            run_id: self.id.clone(),
            parent_run_id: self.parent_id.clone(),
            from,
            to,
            reason,
        });
        log::debug!(
            target: crate::LOG_TARGET,
            "run state transition: run_id={} parent_run_id={} from={from:?} to={to:?} reason={reason:?}",
            self.id,
            self.parent_id_display(),
        );
    }

    pub fn active_step_id(&self) -> Option<RunStepId> {
        self.steps
            .iter()
            .find(|step| step.is_active())
            .map(|step| step.id.clone())
    }

    pub fn begin_step(&mut self) -> Result<RunStepId, RunTransitionError> {
        self.begin_step_with_id(RunStepId::new_v7())
    }

    pub fn begin_step_with_id(
        &mut self,
        step_id: RunStepId,
    ) -> Result<RunStepId, RunTransitionError> {
        if self.status != RunStatus::InvokingModel {
            return Err(RunTransitionError::RunNotActive(self.status));
        }
        if self.steps.iter().any(RunStep::is_active) {
            return Err(RunTransitionError::ActiveStepAlreadyExists);
        }
        self.steps.push(RunStep {
            id: step_id.clone(),
            status: RunStepStatus::Invoking,
            invocation: None,
            tool_calls: Vec::new(),
        });
        self.events.push(RunDomainEvent::StepStarted {
            run_id: self.id.clone(),
            parent_run_id: self.parent_id.clone(),
            step_id: step_id.clone(),
        });
        Ok(step_id)
    }

    pub fn record_model_invocation(
        &mut self,
        step_id: &RunStepId,
        invocation: ModelInvocation,
    ) -> Result<(), RunTransitionError> {
        self.ensure_accepts_step_work()?;
        let step = self.active_step_mut(step_id)?;
        if step.invocation.is_some() {
            return Err(RunTransitionError::InvocationAlreadyRecorded);
        }
        step.invocation = Some(invocation);
        step.status = RunStepStatus::Applying;
        Ok(())
    }

    pub fn add_tool_call(
        &mut self,
        step_id: &RunStepId,
        call: ToolCall,
    ) -> Result<(), RunTransitionError> {
        self.ensure_accepts_step_work()?;
        let step = self.active_step_mut(step_id)?;
        step.tool_calls.push(RunToolCall::new(call));
        step.status = RunStepStatus::ToolPhase;
        Ok(())
    }

    pub fn advance_tool_call(
        &mut self,
        step_id: &RunStepId,
        call_id: &sdk::ids::ToolCallId,
        status: ToolCallStatus,
    ) -> Result<(), RunTransitionError> {
        self.ensure_accepts_step_work()?;
        let step = self.active_step_mut(step_id)?;
        let call = step
            .tool_calls
            .iter_mut()
            .find(|call| call.id() == call_id)
            .ok_or(RunTransitionError::ToolCallNotFound)?;
        let from = call.status();
        if !call.advance(status) {
            return Err(RunTransitionError::IllegalToolCallTransition { from, to: status });
        }
        Ok(())
    }

    fn rejects_controlled_work(&self) -> bool {
        matches!(
            self.status,
            RunStatus::Cancelling
                | RunStatus::CancellingStep
                | RunStatus::FinalizingStep
                | RunStatus::Terminating
        ) || self.status.is_terminal()
    }

    fn ensure_accepts_step_work(&self) -> Result<(), RunTransitionError> {
        if self.rejects_controlled_work() {
            return Err(RunTransitionError::RunNotActive(self.status));
        }
        Ok(())
    }

    fn active_step_mut(&mut self, step_id: &RunStepId) -> Result<&mut RunStep, RunTransitionError> {
        let step = self
            .steps
            .iter_mut()
            .find(|step| &step.id == step_id)
            .ok_or(RunTransitionError::StepNotFound)?;
        if matches!(
            step.status,
            RunStepStatus::Done
                | RunStepStatus::Failed
                | RunStepStatus::Cancelled
                | RunStepStatus::CancellationUnconfirmed
        ) {
            return Err(RunTransitionError::StepNotActive);
        }
        Ok(step)
    }

    pub fn complete_step(&mut self, step_id: &RunStepId) -> Result<(), RunTransitionError> {
        if self.rejects_controlled_work() {
            return Err(RunTransitionError::RunNotActive(self.status));
        }
        let step = self
            .steps
            .iter_mut()
            .find(|step| &step.id == step_id)
            .ok_or(RunTransitionError::StepNotFound)?;
        if !step.is_active() {
            return Err(RunTransitionError::StepNotActive);
        }
        if !step.is_complete() {
            return Err(RunTransitionError::StepIncomplete);
        }
        step.status = RunStepStatus::Done;
        self.events.push(RunDomainEvent::StepCompleted {
            run_id: self.id.clone(),
            parent_run_id: self.parent_id.clone(),
            step_id: step_id.clone(),
        });
        Ok(())
    }

    pub fn mark_stuck(&mut self, reason: impl Into<String>) -> Result<(), RunTransitionError> {
        if self.rejects_controlled_work() {
            return Err(RunTransitionError::RunNotActive(self.status));
        }
        self.events.push(RunDomainEvent::StuckDetected {
            run_id: self.id.clone(),
            parent_run_id: self.parent_id.clone(),
            reason: reason.into(),
        });
        Ok(())
    }

    pub fn complete(&mut self, result: impl Into<String>) -> Result<(), RunTransitionError> {
        if self.steps.iter().any(RunStep::is_active) {
            return Err(RunTransitionError::StepIncomplete);
        }
        self.transition(RunTransition::Finish)?;
        self.events.push(RunDomainEvent::Completed {
            run_id: self.id.clone(),
            parent_run_id: self.parent_id.clone(),
            result: result.into(),
        });
        Ok(())
    }

    pub fn start_draining(&mut self) -> Result<(), RunTransitionError> {
        self.transition(RunTransition::StartDraining)?;
        self.events.push(RunDomainEvent::DrainingInput {
            run_id: self.id.clone(),
            parent_run_id: self.parent_id.clone(),
        });
        Ok(())
    }

    pub fn apply_drain_decision(
        &mut self,
        decision: DrainDecision,
    ) -> Result<(), RunTransitionError> {
        let transition = match decision {
            DrainDecision::Inputs => RunTransition::DrainInputs,
            DrainDecision::InternalContinuation => RunTransition::DrainInternalContinuation,
            DrainDecision::EmptyAndSealed => RunTransition::DrainEmptyAndSealed,
        };
        self.transition(transition)?;
        Ok(())
    }

    pub fn request_step_cancellation(&mut self, step_id: &RunStepId) -> RunStepCancellationRequest {
        if self.status.is_terminal() {
            return RunStepCancellationRequest::RunTerminal;
        }
        if self.status == RunStatus::Terminating {
            return RunStepCancellationRequest::RunTerminating;
        }
        let Some(step) = self.steps.iter_mut().find(|step| &step.id == step_id) else {
            return RunStepCancellationRequest::NoActiveStep;
        };
        if matches!(
            step.status,
            RunStepStatus::Cancelling | RunStepStatus::Finalizing
        ) {
            return RunStepCancellationRequest::AlreadyCancelling;
        }
        if !step.is_active() {
            return RunStepCancellationRequest::NoActiveStep;
        }
        self.pending_interaction = None;
        step.status = RunStepStatus::Cancelling;
        self.apply_state_transition(
            RunStatus::CancellingStep,
            RunTransitionReason::StepCancellationRequested,
        );
        self.events.push(RunDomainEvent::StepCancellationRequested {
            run_id: self.id.clone(),
            parent_run_id: self.parent_id.clone(),
            step_id: step_id.clone(),
        });
        RunStepCancellationRequest::Accepted
    }

    pub fn begin_step_finalization(
        &mut self,
        step_id: &RunStepId,
    ) -> Result<(), RunTransitionError> {
        let step = self.active_step_mut(step_id)?;
        if step.status != RunStepStatus::Cancelling {
            return Err(RunTransitionError::StepNotActive);
        }
        step.status = RunStepStatus::Finalizing;
        self.apply_state_transition(
            RunStatus::FinalizingStep,
            RunTransitionReason::StepFinalizationStarted,
        );
        self.events.push(RunDomainEvent::StepFinalizationStarted {
            run_id: self.id.clone(),
            parent_run_id: self.parent_id.clone(),
            step_id: step_id.clone(),
        });
        Ok(())
    }

    pub fn finish_cancelled_step(&mut self, step_id: &RunStepId) -> Result<(), RunTransitionError> {
        self.finish_controlled_step(step_id, RunStepStatus::Cancelled)
    }

    pub fn finish_unconfirmed_step(
        &mut self,
        step_id: &RunStepId,
    ) -> Result<(), RunTransitionError> {
        self.finish_controlled_step(step_id, RunStepStatus::CancellationUnconfirmed)
    }

    fn finish_controlled_step(
        &mut self,
        step_id: &RunStepId,
        terminal: RunStepStatus,
    ) -> Result<(), RunTransitionError> {
        let step = self.active_step_mut(step_id)?;
        if step.status != RunStepStatus::Finalizing {
            return Err(RunTransitionError::StepNotActive);
        }
        let confirmed = terminal == RunStepStatus::Cancelled;
        step.status = terminal;
        self.transition(RunTransition::StepCancelled)?;
        self.events.push(RunDomainEvent::StepCancelled {
            run_id: self.id.clone(),
            parent_run_id: self.parent_id.clone(),
            step_id: step_id.clone(),
            confirmed,
        });
        self.events.push(RunDomainEvent::DrainingInput {
            run_id: self.id.clone(),
            parent_run_id: self.parent_id.clone(),
        });
        Ok(())
    }

    pub fn request_termination(
        &mut self,
        run_reason: sdk::RunTerminationReason,
        deadline: sdk::ControlDeadline,
    ) -> RunTerminationRequest {
        if self.status.is_terminal() {
            return RunTerminationRequest::AlreadyTerminal;
        }
        if self.status == RunStatus::Terminating {
            return RunTerminationRequest::AlreadyTerminating;
        }
        self.termination = Some((run_reason, deadline));
        self.pending_interaction = None;
        self.apply_state_transition(
            RunStatus::Terminating,
            RunTransitionReason::TerminationRequested,
        );
        self.events.push(RunDomainEvent::TerminationRequested {
            run_id: self.id.clone(),
            parent_run_id: self.parent_id.clone(),
            reason: run_reason,
            deadline,
        });
        RunTerminationRequest::Accepted
    }

    pub fn finish_termination(&mut self) -> Result<(), RunTransitionError> {
        let (reason, _) = self
            .termination
            .ok_or(RunTransitionError::RunNotActive(self.status))?;
        self.transition(RunTransition::TerminationFinished)?;
        self.close_active_steps(RunStepStatus::CancellationUnconfirmed);
        self.events.push(RunDomainEvent::Terminated {
            run_id: self.id.clone(),
            parent_run_id: self.parent_id.clone(),
            reason,
        });
        Ok(())
    }

    pub fn request_cancellation(&mut self) -> RunCancellationRequest {
        if self.status.is_terminal() {
            log::warn!(
                target: crate::LOG_TARGET,
                "run state transition rejected: run_id={} parent_run_id={} from={:?} requested_to={:?} reason={:?}",
                self.id,
                self.parent_id_display(),
                self.status,
                RunStatus::Cancelling,
                RunTransitionReason::InterruptRequested,
            );
            return RunCancellationRequest::AlreadyTerminal;
        }
        if self.status == RunStatus::Cancelling {
            return RunCancellationRequest::AlreadyCancelling;
        }
        self.pending_interaction = None;
        self.apply_state_transition(
            RunStatus::Cancelling,
            RunTransitionReason::InterruptRequested,
        );
        self.events.push(RunDomainEvent::CancellationRequested {
            run_id: self.id.clone(),
            parent_run_id: self.parent_id.clone(),
        });
        RunCancellationRequest::Accepted
    }

    pub fn finish_cancellation(&mut self) -> Result<(), RunTransitionError> {
        self.transition(RunTransition::CancellationFinished)?;
        self.close_active_steps(RunStepStatus::Cancelled);
        self.events.push(RunDomainEvent::Cancelled {
            run_id: self.id.clone(),
            parent_run_id: self.parent_id.clone(),
        });
        Ok(())
    }

    fn close_active_steps(&mut self, status: RunStepStatus) {
        for step in &mut self.steps {
            if !matches!(
                step.status,
                RunStepStatus::Done
                    | RunStepStatus::Failed
                    | RunStepStatus::Cancelled
                    | RunStepStatus::CancellationUnconfirmed
            ) {
                step.status = status;
            }
        }
    }

    pub fn fail(&mut self, error: impl Into<String>) -> Result<(), RunTransitionError> {
        if self.rejects_controlled_work() {
            log::warn!(
                target: crate::LOG_TARGET,
                "run state transition rejected: run_id={} parent_run_id={} from={:?} requested_to={:?} reason={:?}",
                self.id,
                self.parent_id_display(),
                self.status,
                RunStatus::Failed,
                RunTransitionReason::Failed,
            );
            return Err(RunTransitionError::RunNotActive(self.status));
        }
        self.apply_state_transition(RunStatus::Failed, RunTransitionReason::Failed);
        self.close_active_steps(RunStepStatus::Failed);
        self.events.push(RunDomainEvent::Failed {
            run_id: self.id.clone(),
            parent_run_id: self.parent_id.clone(),
            error: error.into(),
        });
        Ok(())
    }
}
