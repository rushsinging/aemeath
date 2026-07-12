use tokio_util::sync::CancellationToken;

mod domain;

pub trait ActiveRunPort: Send + Sync {
    fn activate(&self, run_id: RunId, cancel: CancellationToken);
    fn claim_terminal(&self, run_id: &RunId) -> bool;
    fn clear(&self, run_id: &RunId);
}

pub use domain::{
    Run, RunCancellationRequest, RunDomainEvent, RunId, RunSpec, RunStatus, RunStep, RunStepId,
    RunStepStatus, RunTransition, RunTransitionError,
};

#[cfg(test)]
mod tests;
