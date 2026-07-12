use std::time::{Duration, Instant};

use uuid::Uuid;

pub use sdk::RunId;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RunStepId(Uuid);

impl RunStepId {
    fn new_v7() -> Self {
        Self(Uuid::now_v7())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunSpec {
    pub name: String,
    pub timeout: Duration,
}

impl RunSpec {
    pub fn new(name: impl Into<String>, timeout: Duration) -> Self {
        Self {
            name: name.into(),
            timeout,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunStatus {
    Created,
    PreparingContext,
    InvokingModel,
    ApplyingResponse,
    AwaitingToolApproval,
    ExecutingTools,
    AwaitingUser,
    Compacting,
    Finishing,
    Cancelling,
    Completed,
    Failed,
    Cancelled,
}

impl RunStatus {
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunStepStatus {
    Invoking,
    Applying,
    ToolPhase,
    Done,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunStep {
    id: RunStepId,
    status: RunStepStatus,
}

impl RunStep {
    pub fn id(&self) -> &RunStepId {
        &self.id
    }

    pub fn status(&self) -> RunStepStatus {
        self.status
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunTransition {
    Start,
    BeginCompaction,
    CompactionCompleted,
    ContextPrepared,
    ModelInvoked,
    ResponseWithTools,
    ResponseWithoutTools,
    ContinueAfterResponse,
    ToolsApproved,
    AwaitUser,
    UserResumed,
    ToolsCompleted,
    Finish,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RunTransitionError {
    #[error("illegal Run transition: {from:?} --{transition:?}-->")]
    IllegalTransition {
        from: RunStatus,
        transition: RunTransition,
    },
    #[error("Run is not active: {0:?}")]
    RunNotActive(RunStatus),
    #[error("Run step not found")]
    StepNotFound,
    #[error("Run step is not active")]
    StepNotActive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunCancellationRequest {
    Accepted,
    AlreadyCancelling,
    AlreadyTerminal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunDomainEvent {
    Started {
        run_id: RunId,
        parent_run_id: Option<RunId>,
    },
    StepStarted {
        run_id: RunId,
        parent_run_id: Option<RunId>,
        step_id: RunStepId,
    },
    StepCompleted {
        run_id: RunId,
        parent_run_id: Option<RunId>,
        step_id: RunStepId,
    },
    CancellationRequested {
        run_id: RunId,
        parent_run_id: Option<RunId>,
    },
    AwaitingUser {
        run_id: RunId,
        parent_run_id: Option<RunId>,
    },
    Resumed {
        run_id: RunId,
        parent_run_id: Option<RunId>,
    },
    StuckDetected {
        run_id: RunId,
        parent_run_id: Option<RunId>,
        reason: String,
    },
    Completed {
        run_id: RunId,
        parent_run_id: Option<RunId>,
        result: String,
    },
    Failed {
        run_id: RunId,
        parent_run_id: Option<RunId>,
        error: String,
    },
    Cancelled {
        run_id: RunId,
        parent_run_id: Option<RunId>,
    },
}

impl RunDomainEvent {
    pub fn parent_run_id(&self) -> Option<&RunId> {
        match self {
            Self::Started { parent_run_id, .. }
            | Self::StepStarted { parent_run_id, .. }
            | Self::StepCompleted { parent_run_id, .. }
            | Self::CancellationRequested { parent_run_id, .. }
            | Self::AwaitingUser { parent_run_id, .. }
            | Self::Resumed { parent_run_id, .. }
            | Self::StuckDetected { parent_run_id, .. }
            | Self::Completed { parent_run_id, .. }
            | Self::Failed { parent_run_id, .. }
            | Self::Cancelled { parent_run_id, .. } => parent_run_id.as_ref(),
        }
    }
}

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
        Self {
            id: RunId::new_v7(),
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
        let next = match (self.status, transition) {
            (RunStatus::Created, RunTransition::Start) => RunStatus::PreparingContext,
            (RunStatus::PreparingContext, RunTransition::BeginCompaction) => RunStatus::Compacting,
            (RunStatus::Compacting, RunTransition::CompactionCompleted) => {
                RunStatus::PreparingContext
            }
            (RunStatus::PreparingContext, RunTransition::ContextPrepared) => {
                RunStatus::InvokingModel
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
        let step_id = RunStepId::new_v7();
        self.steps.push(RunStep {
            id: step_id.clone(),
            status: RunStepStatus::Invoking,
        });
        self.events.push(RunDomainEvent::StepStarted {
            run_id: self.id.clone(),
            parent_run_id: self.parent_id.clone(),
            step_id: step_id.clone(),
        });
        Ok(step_id)
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
        if step.status == RunStepStatus::Done {
            return Err(RunTransitionError::StepNotActive);
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
        if self.status != RunStatus::Cancelling {
            return Err(RunTransitionError::RunNotActive(self.status));
        }
        self.status = RunStatus::Cancelled;
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
