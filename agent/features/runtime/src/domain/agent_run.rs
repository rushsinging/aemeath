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
    fn activate_child(&self, run_id: RunId, cancel: CancellationToken);
    fn activate_main(&self, run_id: RunId, cancel: CancellationToken);
    fn set_main_active_step(
        &self,
        _run_id: &RunId,
        _step_id: RunStepId,
        _cancel: CancellationToken,
    ) {
    }
    fn clear_main_active_step(&self, _run_id: &RunId, _step_id: &RunStepId) {}
    fn take_control(&self, _run_id: &RunId) -> Option<RunControl> {
        None
    }
    fn clear(&self, run_id: &RunId);
}

pub use domain::Run;
#[cfg(test)]
pub use event::RunTimingSnapshot;
pub use event::{RunId, RuntimeLifecycleEvent};
#[cfg(test)]
pub use spec::{
    EventRoute, InputMode, InteractionMode, MemoryMode, ReasoningBindingMode, ResourceMode,
    ToolScope,
};
pub use spec::{HookBindingMode, InteractionBindingMode, RunSpec, RunSpecError};
pub use state::{
    DrainDecision, InteractionContinuation, RunStatus, RunStepCancellationRequest, RunStepId,
    RunStepStatus, RunTerminationRequest, RunTransition, RunTransitionError, StopHookBlockResult,
};
#[cfg(test)]
pub use state::{PendingInteraction, RunTransitionReason};
pub use step::{ModelInvocation, ToolCall, ToolCallStatus};

#[cfg(test)]
mod tests;
