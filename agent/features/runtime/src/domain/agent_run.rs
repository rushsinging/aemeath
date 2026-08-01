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
    fn activate_session(&self, run_id: RunId, cancel: CancellationToken);
    fn set_active_step(&self, _run_id: &RunId, _step_id: RunStepId, _cancel: CancellationToken) {}
    fn clear_active_step(&self, _run_id: &RunId, _step_id: &RunStepId) {}
    fn clear_cancelled_step(&self, run_id: &RunId, step_id: &RunStepId) {
        self.clear_active_step(run_id, step_id);
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
#[cfg(test)]
pub use spec::{
    EventRoute, InputMode, InteractionMode, MemoryMode, ReasoningBindingMode, ResourceMode,
    ToolScope,
};
pub use spec::{HookBindingMode, InteractionBindingMode, RunSpec, RunSpecError};
pub use state::{
    DrainDecision, InteractionContinuation, RunCancellationRequest, RunStatus,
    RunStepCancellationRequest, RunStepId, RunTerminationRequest, RunTransition,
    RunTransitionError, StopHookBlockResult,
};
#[cfg(test)]
pub use state::{PendingInteraction, RunStepStatus, RunTransitionReason};
pub use step::{ModelInvocation, ToolCall, ToolCallStatus};

#[cfg(test)]
mod tests;
