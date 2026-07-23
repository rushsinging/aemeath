use tokio_util::sync::CancellationToken;

mod domain;
mod event;
mod spec;
mod state;
mod step;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunControl {
    CancelStep {
        step_id: RunStepId,
        deadline: sdk::ControlDeadline,
    },
    Terminate {
        reason: sdk::RunTerminationReason,
        deadline: sdk::ControlDeadline,
    },
}

pub trait ActiveRunPort: Send + Sync {
    fn activate(&self, run_id: RunId, cancel: CancellationToken);
    fn activate_main(&self, run_id: RunId, cancel: CancellationToken) {
        self.activate(run_id, cancel);
    }
    fn set_main_active_step(
        &self,
        _run_id: &RunId,
        _step_id: RunStepId,
        _cancel: CancellationToken,
    ) {
    }
    fn take_control(&self, _run_id: &RunId) -> Option<RunControl> {
        None
    }
    fn claim_terminal(&self, run_id: &RunId) -> bool;
    fn claim_cancellation(&self, run_id: &RunId) -> bool;
    fn clear(&self, run_id: &RunId);
}

pub use domain::Run;
pub use event::{RunDomainEvent, RunId};
pub use spec::{
    EventRoute, InputMode, InteractionMode, MemoryMode, ResourceMode, RunKind, RunSpec,
    RunSpecError, ToolScope,
};
pub use state::{
    DrainDecision, InteractionContinuation, PendingInteraction, RunCancellationRequest, RunStatus,
    RunStep, RunStepCancellationRequest, RunStepId, RunStepStatus, RunTerminationRequest,
    RunTransition, RunTransitionError, RunTransitionReason,
};
pub use step::{ModelInvocation, RunToolCall, ToolCall, ToolCallStatus};

#[cfg(test)]
mod tests;
