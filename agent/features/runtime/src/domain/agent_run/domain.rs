use std::time::{Duration, Instant};

use crate::domain::agent_run::ToolCall;

use super::event::{RunDomainEvent, RunId};
use super::spec::RunSpec;
use super::state::{
    RunCancellationRequest, RunStatus, RunStep, RunStepId, RunStepStatus, RunTransition,
    RunTransitionError,
};
use super::step::{ModelInvocation, RunToolCall, ToolCallStatus};

#[derive(Debug)]
pub struct Run {
    id: RunId,
    spec: RunSpec,
    parent_id: Option<RunId>,
    status: RunStatus,
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
            (RunStatus::Cancelling, RunTransition::CancellationFinished) => RunStatus::Cancelled,
            (from, transition) => {
                return Err(RunTransitionError::IllegalTransition { from, transition });
            }
        };

        self.status = next;
        if transition == RunTransition::Start {
            self.started_at = Some(Instant::now());
            self.events.push(RunDomainEvent::Started {
                run_id: self.id.clone(),
                parent_run_id: self.parent_id.clone(),
            });
        } else if next == RunStatus::AwaitingUser {
            self.events.push(RunDomainEvent::AwaitingUser {
                run_id: self.id.clone(),
                parent_run_id: self.parent_id.clone(),
            });
        } else if transition == RunTransition::UserResumed {
            self.events.push(RunDomainEvent::Resumed {
                run_id: self.id.clone(),
                parent_run_id: self.parent_id.clone(),
            });
        }
        Ok(next)
    }

    pub fn begin_step(&mut self) -> Result<RunStepId, RunTransitionError> {
        if self.status != RunStatus::InvokingModel {
            return Err(RunTransitionError::RunNotActive(self.status));
        }
        if self.steps.iter().any(RunStep::is_active) {
            return Err(RunTransitionError::ActiveStepAlreadyExists);
        }
        let step_id = RunStepId::new_v7();
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

    fn ensure_accepts_step_work(&self) -> Result<(), RunTransitionError> {
        if self.status.is_terminal() || self.status == RunStatus::Cancelling {
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
            RunStepStatus::Done | RunStepStatus::Failed | RunStepStatus::Cancelled
        ) {
            return Err(RunTransitionError::StepNotActive);
        }
        Ok(step)
    }

    pub fn complete_step(&mut self, step_id: &RunStepId) -> Result<(), RunTransitionError> {
        if self.status.is_terminal() || self.status == RunStatus::Cancelling {
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
        if self.status.is_terminal() || self.status == RunStatus::Cancelling {
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

    pub fn request_cancellation(&mut self) -> RunCancellationRequest {
        if self.status.is_terminal() {
            return RunCancellationRequest::AlreadyTerminal;
        }
        if self.status == RunStatus::Cancelling {
            return RunCancellationRequest::AlreadyCancelling;
        }
        self.status = RunStatus::Cancelling;
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
                RunStepStatus::Done | RunStepStatus::Failed | RunStepStatus::Cancelled
            ) {
                step.status = status;
            }
        }
    }

    pub fn fail(&mut self, error: impl Into<String>) -> Result<(), RunTransitionError> {
        if self.status.is_terminal() || self.status == RunStatus::Cancelling {
            return Err(RunTransitionError::RunNotActive(self.status));
        }
        self.status = RunStatus::Failed;
        self.close_active_steps(RunStepStatus::Failed);
        self.events.push(RunDomainEvent::Failed {
            run_id: self.id.clone(),
            parent_run_id: self.parent_id.clone(),
            error: error.into(),
        });
        Ok(())
    }
}
