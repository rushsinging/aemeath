use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunControl {
    CancelStep,
    Terminate {
        reason: sdk::RunTerminationReason,
        deadline: sdk::ControlDeadline,
    },
}

mod domain;
mod event;
mod spec;
mod state;
mod step;

pub trait ActiveRunPort: Send + Sync {
    fn activate(&self, run_id: RunId, parent_run_id: Option<RunId>, cancel: CancellationToken);
    fn claim_terminal(&self, run_id: &RunId) -> bool;
    fn claim_cancellation(&self, run_id: &RunId) -> bool;
    fn take_control(&self, _run_id: &RunId) -> Option<RunControl> {
        None
    }
    fn take_legacy_cancellation(&self, _run_id: &RunId) -> bool {
        false
    }
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
