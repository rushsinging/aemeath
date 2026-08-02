use super::ActivityCoordinator;
use crate::domain::agent_run::{Run, RunStatus, RunTransition};
use sdk::{ActivityKindView, ActivityStateView, RunId, RunPhaseKindView};
use std::sync::{Arc, Mutex};

use super::coordinator::{ActivityClock, ActivityIdSource};

#[derive(Clone)]
struct FixedActivityClock(Arc<Mutex<u64>>);

impl FixedActivityClock {
    fn new() -> Self {
        Self(Arc::new(Mutex::new(1_000)))
    }
}

impl ActivityClock for FixedActivityClock {
    fn now_monotonic_ms(&self) -> u64 {
        *self.0.lock().expect("fixed clock lock")
    }

    fn now_unix_ms(&self) -> u64 {
        self.now_monotonic_ms()
    }
}

#[derive(Default)]
struct FixedActivityIdSource {
    next: Mutex<u64>,
}

impl ActivityIdSource for FixedActivityIdSource {
    fn next_activity_id(&self) -> sdk::ActivityId {
        let mut next = self.next.lock().expect("fixed activity id lock");
        *next += 1;
        sdk::ActivityId::new(format!("run-activity-{next}"))
    }
}

fn coordinator(run_id: &RunId) -> ActivityCoordinator {
    ActivityCoordinator::new(
        run_id.clone(),
        Arc::new(FixedActivityClock::new()),
        Arc::new(FixedActivityIdSource::default()),
    )
}

fn observe_draining(coordinator: &ActivityCoordinator, run: &mut Run) {
    run.start_draining().expect("start draining");
    coordinator
        .observe_run_events(&run.drain_events())
        .expect("observe draining events");
}

#[test]
fn started_run_creates_one_root_and_one_draining_phase() {
    let mut run = Run::with_id(
        RunId::new("run-root"),
        crate::domain::agent_run::RunSpec::main(),
        None,
    );
    let coordinator = coordinator(run.id());

    observe_draining(&coordinator, &mut run);

    let snapshot = coordinator.snapshot();
    assert_eq!(snapshot.activities.len(), 2);
    let root = snapshot
        .activities
        .iter()
        .find(|activity| activity.kind == ActivityKindView::Run)
        .expect("run root activity");
    let phase = snapshot
        .activities
        .iter()
        .find(|activity| {
            activity.kind == ActivityKindView::RunPhase(RunPhaseKindView::DrainingInput)
        })
        .expect("draining phase activity");
    assert_eq!(root.state, ActivityStateView::Running);
    assert_eq!(phase.parent_activity_id.as_ref(), Some(&root.id));
    assert_eq!(phase.state, ActivityStateView::Running);
}

#[test]
fn transition_finishes_old_phase_before_starting_new_phase() {
    let mut run = Run::with_id(
        RunId::new("run-phase"),
        crate::domain::agent_run::RunSpec::main(),
        None,
    );
    let coordinator = coordinator(run.id());
    observe_draining(&coordinator, &mut run);

    run.transition(RunTransition::DrainInputs)
        .expect("prepare context");
    coordinator
        .observe_run_events(&run.drain_events())
        .expect("observe preparing context");

    let snapshot = coordinator.snapshot();
    let draining = snapshot
        .activities
        .iter()
        .find(|activity| {
            activity.kind == ActivityKindView::RunPhase(RunPhaseKindView::DrainingInput)
        })
        .expect("draining phase");
    let preparing = snapshot
        .activities
        .iter()
        .find(|activity| {
            activity.kind == ActivityKindView::RunPhase(RunPhaseKindView::PreparingContext)
        })
        .expect("preparing phase");
    assert_eq!(draining.state, ActivityStateView::Succeeded);
    assert_eq!(preparing.state, ActivityStateView::Running);
    assert!(draining.revision < preparing.revision);
    assert_eq!(snapshot.revision, 4);
}

#[test]
fn model_owned_run_status_closes_phase_without_creating_duplicate_phase() {
    let mut run = Run::with_id(
        RunId::new("run-model"),
        crate::domain::agent_run::RunSpec::main(),
        None,
    );
    let coordinator = coordinator(run.id());
    observe_draining(&coordinator, &mut run);
    run.transition(RunTransition::DrainInputs)
        .expect("prepare context");
    coordinator
        .observe_run_events(&run.drain_events())
        .expect("observe preparing context");

    run.transition(RunTransition::ContextPrepared)
        .expect("invoke model");
    coordinator
        .observe_run_events(&run.drain_events())
        .expect("observe model status");

    let snapshot = coordinator.snapshot();
    assert!(snapshot
        .activities
        .iter()
        .filter(|activity| !activity.state.is_terminal())
        .all(|activity| activity.kind == ActivityKindView::Run));
    assert!(!snapshot.activities.iter().any(|activity| {
        activity.kind == ActivityKindView::RunPhase(RunPhaseKindView::PreparingContext)
            && activity.state == ActivityStateView::Running
    }));
}

#[test]
fn completed_run_finishes_root_and_leaves_no_live_activity() {
    let run_id = RunId::new("run-complete");
    let coordinator = coordinator(&run_id);
    coordinator
        .observe_run_events(&super::run_events::terminal_events_for_test(
            run_id,
            RunStatus::Completed,
        ))
        .expect("observe completion");

    let snapshot = coordinator.snapshot();
    assert!(snapshot
        .activities
        .iter()
        .all(|activity| activity.state.is_terminal()));
    assert_eq!(
        snapshot
            .activities
            .iter()
            .find(|activity| activity.kind == ActivityKindView::Run)
            .expect("run root")
            .state,
        ActivityStateView::Succeeded
    );
}

#[test]
fn terminal_cause_is_not_projected_as_success() {
    for (status, expected) in [
        (RunStatus::Failed, ActivityStateView::Failed),
        (RunStatus::Cancelled, ActivityStateView::Cancelled),
        (RunStatus::Terminated, ActivityStateView::Terminated),
    ] {
        let run_id = RunId::new(format!("terminal-{status:?}"));
        let coordinator = coordinator(&run_id);
        let events = super::run_events::terminal_events_for_test(run_id, status);
        coordinator
            .observe_run_events(&events)
            .expect("observe terminal run events");
        let snapshot = coordinator.snapshot();
        assert!(snapshot
            .activities
            .iter()
            .all(|activity| activity.state == expected));
        assert!(!snapshot
            .activities
            .iter()
            .any(|activity| activity.state == ActivityStateView::Succeeded));
    }
}

trait ActivityStateTerminal {
    fn is_terminal(&self) -> bool;
}

impl ActivityStateTerminal for ActivityStateView {
    fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Succeeded | Self::Failed | Self::Cancelled | Self::Terminated
        )
    }
}
