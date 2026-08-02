use tokio_util::sync::CancellationToken;

use crate::domain::agent_run::ActiveRunPort;

pub(crate) trait StepScopeObserver: Send + Sync {
    fn register_step_scope(
        &self,
        _run_id: &sdk::RunId,
        _step_id: sdk::RunStepId,
        _cancel: CancellationToken,
    ) {
    }
}

pub(crate) struct NoopStepScopeObserver;

impl StepScopeObserver for NoopStepScopeObserver {}

pub(crate) struct ActiveStepScopeObserver<'a> {
    active_run: &'a dyn ActiveRunPort,
}

impl<'a> ActiveStepScopeObserver<'a> {
    pub(crate) fn new(active_run: &'a dyn ActiveRunPort) -> Self {
        Self { active_run }
    }
}

impl StepScopeObserver for ActiveStepScopeObserver<'_> {
    fn register_step_scope(
        &self,
        run_id: &sdk::RunId,
        step_id: sdk::RunStepId,
        cancel: CancellationToken,
    ) {
        self.active_run
            .set_main_active_step(run_id, step_id, cancel);
    }
}

pub(crate) struct RunLifecycleCoordinator<O> {
    step_scope: O,
}

impl<O> RunLifecycleCoordinator<O>
where
    O: StepScopeObserver,
{
    pub(crate) fn new(_active_run: &dyn ActiveRunPort, step_scope: O) -> Self {
        Self { step_scope }
    }

    pub(crate) fn register_step_scope(
        &self,
        run_id: &sdk::RunId,
        step_id: sdk::RunStepId,
        cancel: CancellationToken,
    ) {
        self.step_scope.register_step_scope(run_id, step_id, cancel);
    }
}

#[cfg(test)]
#[path = "run_lifecycle_tests.rs"]
mod tests;
