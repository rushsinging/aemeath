use tokio_util::sync::CancellationToken;

mod domain;
mod event;
mod spec;
mod state;
mod step;

pub trait ActiveRunPort: Send + Sync {
    fn activate(&self, run_id: RunId, cancel: CancellationToken);
    fn claim_terminal(&self, run_id: &RunId) -> bool;
    fn claim_cancellation(&self, run_id: &RunId) -> bool;
    fn clear(&self, run_id: &RunId);
}

pub use domain::Run;
pub use event::{RunDomainEvent, RunId};
pub use spec::{
    EventRoute, InputMode, InteractionMode, ResourceMode, RunKind, RunSpec, RunSpecError, ToolScope,
};
pub use state::{
    RunCancellationRequest, RunStatus, RunStep, RunStepId, RunStepStatus, RunTransition,
    RunTransitionError,
};
pub use step::{ModelInvocation, RunToolCall, ToolCall, ToolCallStatus};

#[cfg(test)]
mod tests;
