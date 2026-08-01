use tokio_util::sync::CancellationToken;

use crate::application::loop_engine::{
    LoopEngineError, PlanApprovalPort, RunControlPort, RunLifecyclePort, StuckDecision,
    StuckHandlingPort,
};
use crate::application::run::execution_state::RunExecutionState;
use crate::domain::agent_run::ActiveRunPort;

pub(crate) struct ActiveRunControl<'a> {
    active_run: &'a dyn ActiveRunPort,
    run_id: &'a sdk::RunId,
}

impl<'a> ActiveRunControl<'a> {
    pub(crate) fn new(active_run: &'a dyn ActiveRunPort, run_id: &'a sdk::RunId) -> Self {
        Self { active_run, run_id }
    }
}

impl RunControlPort for ActiveRunControl<'_> {
    fn take_control(&self, run_id: &sdk::RunId) -> Option<crate::domain::agent_run::RunControl> {
        debug_assert_eq!(run_id, self.run_id);
        self.active_run.take_control(run_id)
    }
}

pub(crate) struct NoopRunControl;

impl RunControlPort for NoopRunControl {
    fn take_control(&self, _run_id: &sdk::RunId) -> Option<crate::domain::agent_run::RunControl> {
        None
    }
}

pub(crate) enum StepScopeRegistration<'a> {
    Active(&'a dyn ActiveRunPort),
    Disabled,
}

pub(crate) struct ActiveRunLifecycle<'a> {
    active_run: &'a dyn ActiveRunPort,
    step_scope: StepScopeRegistration<'a>,
}

impl<'a> ActiveRunLifecycle<'a> {
    pub(crate) fn new(
        active_run: &'a dyn ActiveRunPort,
        step_scope: StepScopeRegistration<'a>,
    ) -> Self {
        Self {
            active_run,
            step_scope,
        }
    }
}

impl RunLifecyclePort for ActiveRunLifecycle<'_> {
    fn claim_terminal(&self, run_id: &sdk::RunId) -> bool {
        self.active_run.claim_terminal(run_id)
    }

    fn claim_cancellation(&self, run_id: &sdk::RunId) -> bool {
        self.active_run.claim_cancellation(run_id)
    }

    fn register_step_scope(
        &self,
        run_id: &sdk::RunId,
        step_id: sdk::RunStepId,
        cancel: CancellationToken,
    ) {
        if let StepScopeRegistration::Active(active_run) = self.step_scope {
            active_run.set_active_step(run_id, step_id, cancel);
        }
    }

    fn clear_step_scope(&self, run_id: &sdk::RunId, step_id: &sdk::RunStepId) {
        if let StepScopeRegistration::Active(active_run) = self.step_scope {
            active_run.clear_active_step(run_id, step_id);
        }
    }

    fn clear_cancelled_step_scope(&self, run_id: &sdk::RunId, step_id: &sdk::RunStepId) {
        if let StepScopeRegistration::Active(active_run) = self.step_scope {
            active_run.clear_cancelled_step(run_id, step_id);
        }
    }
}

pub(crate) struct FixedPlanApproval {
    required: bool,
}

impl FixedPlanApproval {
    pub(crate) fn new(required: bool) -> Self {
        Self { required }
    }
}

impl PlanApprovalPort for FixedPlanApproval {
    fn needs_plan_approval(&self) -> bool {
        self.required
    }
}

pub(crate) struct NoopStuckObserver;

#[async_trait::async_trait]
impl StuckHandlingPort for NoopStuckObserver {
    async fn on_stuck(
        &mut self,
        _execution: &RunExecutionState,
        _decision: &StuckDecision,
    ) -> Result<(), LoopEngineError> {
        Ok(())
    }
}
