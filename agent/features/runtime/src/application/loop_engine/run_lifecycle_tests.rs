use std::sync::Mutex;

use super::*;
use crate::domain::agent_run::{RunControl, RunId};

#[derive(Default)]
struct RecordingActiveRun {
    step_scopes: Mutex<Vec<(RunId, sdk::RunStepId)>>,
}

impl ActiveRunPort for RecordingActiveRun {
    fn activate(&self, _run_id: RunId, _cancel: CancellationToken) {}

    fn activate_main(&self, _run_id: RunId, _cancel: CancellationToken) {}

    fn set_main_active_step(
        &self,
        run_id: &RunId,
        step_id: sdk::RunStepId,
        _cancel: CancellationToken,
    ) {
        self.step_scopes
            .lock()
            .unwrap()
            .push((run_id.clone(), step_id));
    }

    fn take_control(&self, _run_id: &RunId) -> Option<RunControl> {
        None
    }

    fn clear(&self, _run_id: &RunId) {}
}

#[test]
fn main_step_scope_observer_registers_active_step() {
    let active_run = RecordingActiveRun::default();
    let coordinator =
        RunLifecycleCoordinator::new(&active_run, ActiveStepScopeObserver::new(&active_run));
    let run_id = RunId::new_v7();
    let step_id = sdk::RunStepId::new_v7();

    coordinator.register_step_scope(&run_id, step_id.clone(), CancellationToken::new());

    assert_eq!(
        active_run.step_scopes.lock().unwrap().as_slice(),
        &[(run_id, step_id)]
    );
}

#[test]
fn noop_step_scope_observer_does_not_register_active_step() {
    let active_run = RecordingActiveRun::default();
    let coordinator = RunLifecycleCoordinator::new(&active_run, NoopStepScopeObserver);

    coordinator.register_step_scope(
        &RunId::new_v7(),
        sdk::RunStepId::new_v7(),
        CancellationToken::new(),
    );

    assert!(active_run.step_scopes.lock().unwrap().is_empty());
}
