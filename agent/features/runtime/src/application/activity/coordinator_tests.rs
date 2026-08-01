use super::coordinator::{
    ActivityClock, ActivityCoordinator, ActivityIdSource, ActivityTerminal, StartActivity,
    UpdateActivity,
};
use super::model::{ActivityDetail, ActivityKind, ActivitySource, RunPhaseKind};
use sdk::{ActivityAudienceView, ActivityId, RunId};
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct FixedActivityIdState {
    next: u64,
}

#[derive(Clone, Default)]
struct FixedActivityIdSource(Arc<Mutex<FixedActivityIdState>>);

#[derive(Clone)]
struct FixedActivityClock(Arc<Mutex<u64>>);

impl FixedActivityClock {
    fn new() -> Self {
        Self(Arc::new(Mutex::new(1_000)))
    }

    fn advance_ms(&self, elapsed_ms: u64) {
        *self.0.lock().expect("fixed clock lock") += elapsed_ms;
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

impl ActivityIdSource for FixedActivityIdSource {
    fn next_activity_id(&self) -> ActivityId {
        let mut state = self.0.lock().expect("fixed activity id lock");
        state.next += 1;
        ActivityId::new(format!("activity-from-test-{}", state.next))
    }
}

fn coordinator() -> (ActivityCoordinator, FixedActivityClock) {
    let clock = FixedActivityClock::new();
    let coordinator = ActivityCoordinator::new(
        RunId::new("run-1"),
        Arc::new(clock.clone()),
        Arc::new(FixedActivityIdSource::default()),
    );
    (coordinator, clock)
}

fn start_tool() -> StartActivity {
    StartActivity {
        run_step_id: None,
        parent_activity_id: None,
        source: ActivitySource::Run,
        kind: ActivityKind::Run,
        detail: ActivityDetail::Run,
        audience: ActivityAudienceView::User,
    }
}

#[test]
fn start_assigns_identity_and_revision_one() {
    let (coordinator, _) = coordinator();
    let activity_id = coordinator.start(start_tool()).expect("start activity");
    let observation = coordinator
        .snapshot()
        .activities
        .into_iter()
        .find(|activity| activity.id == activity_id)
        .expect("started activity");

    assert_eq!(observation.revision, 1);
    assert_eq!(observation.state, sdk::ActivityStateView::Running);
    assert_eq!(
        observation.timing,
        sdk::ActivityTimingView {
            total_elapsed_ms: 0,
            active_elapsed_ms: 0,
            state_elapsed_ms: 0,
            started_at_unix_ms: Some(1_000),
            finished_at_unix_ms: None,
        }
    );
}

#[test]
fn update_replaces_detail_and_advances_revision() {
    let (coordinator, _) = coordinator();
    let activity_id = coordinator.start(start_tool()).expect("start activity");

    coordinator
        .update(UpdateActivity {
            activity_id: activity_id.clone(),
            detail: Some(ActivityDetail::Model {
                model: "test-model".to_string(),
                attempt: 2,
                stream: sdk::ModelStreamStateView::Retrying,
            }),
        })
        .expect("update activity");

    let observation = coordinator
        .snapshot()
        .find(&activity_id)
        .expect("updated activity")
        .clone();
    assert_eq!(observation.revision, 2);
    assert_eq!(
        observation.detail,
        sdk::ActivityDetailView::Model {
            model: "test-model".to_string(),
            attempt: 2,
            stream: sdk::ModelStreamStateView::Retrying,
        }
    );
}

#[test]
fn waiting_pauses_active_time_but_total_time_continues() {
    let (coordinator, clock) = coordinator();
    let activity_id = coordinator.start(start_tool()).expect("start activity");
    clock.advance_ms(2_000);
    coordinator
        .wait(UpdateActivity {
            activity_id: activity_id.clone(),
            detail: None,
        })
        .expect("wait activity");
    clock.advance_ms(3_000);

    let observation = coordinator
        .snapshot()
        .activities
        .into_iter()
        .find(|activity| activity.id == activity_id)
        .expect("waiting activity");
    assert_eq!(observation.state, sdk::ActivityStateView::Waiting);
    assert_eq!(observation.timing.total_elapsed_ms, 5_000);
    assert_eq!(observation.timing.active_elapsed_ms, 2_000);
}

#[test]
fn finish_freezes_timing_and_terminal_state() {
    let (coordinator, clock) = coordinator();
    let activity_id = coordinator.start(start_tool()).expect("start activity");
    clock.advance_ms(2_000);
    coordinator
        .finish(activity_id.clone(), ActivityTerminal::Succeeded)
        .expect("finish activity");
    clock.advance_ms(3_000);

    let observation = coordinator
        .snapshot()
        .activities
        .into_iter()
        .find(|activity| activity.id == activity_id)
        .expect("finished activity");
    assert_eq!(observation.state, sdk::ActivityStateView::Succeeded);
    assert_eq!(observation.timing.total_elapsed_ms, 2_000);
    assert_eq!(observation.timing.active_elapsed_ms, 2_000);
    assert_eq!(observation.timing.finished_at_unix_ms, Some(3_000));
}

#[test]
fn close_run_never_converts_live_activity_to_success() {
    let (coordinator, _) = coordinator();
    let activity_id = coordinator.start(start_tool()).expect("start activity");
    coordinator
        .close_run(ActivityTerminal::Terminated)
        .expect("close run");

    let observation = coordinator
        .snapshot()
        .activities
        .into_iter()
        .find(|activity| activity.id == activity_id)
        .expect("closed activity");
    assert_eq!(observation.state, sdk::ActivityStateView::Terminated);
}

#[test]
fn terminal_activity_rejects_conflicting_update() {
    let (coordinator, _) = coordinator();
    let activity_id = coordinator.start(start_tool()).expect("start activity");
    coordinator
        .finish(activity_id.clone(), ActivityTerminal::Succeeded)
        .expect("finish activity");

    let error = coordinator
        .wait(UpdateActivity {
            activity_id,
            detail: None,
        })
        .expect_err("terminal update must fail");
    assert!(error.to_string().contains("终态"));
}

#[test]
fn duplicate_live_source_is_rejected() {
    let (coordinator, _) = coordinator();
    coordinator.start(start_tool()).expect("first activity");

    let error = coordinator
        .start(start_tool())
        .expect_err("duplicate live source must fail");
    assert!(error.to_string().contains("同一来源"));
}

#[test]
fn parent_activity_must_exist_and_be_live() {
    let (coordinator, _) = coordinator();
    let missing_parent_id = ActivityId::new("missing-parent");
    let child_step_id = sdk::RunStepId::new_v7();
    let mut child = start_tool();
    child.source = ActivitySource::RunStep(child_step_id.clone());
    child.run_step_id = Some(child_step_id);
    child.parent_activity_id = Some(missing_parent_id);

    let error = coordinator
        .start(child)
        .expect_err("missing parent must fail");
    assert!(error.to_string().contains("父节点不存在"));
}

#[test]
fn source_run_step_must_match_activity_run_step() {
    let (coordinator, _) = coordinator();
    let source_step_id = sdk::RunStepId::new_v7();
    let command_step_id = sdk::RunStepId::new_v7();
    let command = StartActivity {
        run_step_id: Some(command_step_id),
        parent_activity_id: None,
        source: ActivitySource::RunStep(source_step_id),
        kind: ActivityKind::RunPhase(RunPhaseKind::PreparingContext),
        detail: ActivityDetail::Phase(RunPhaseKind::PreparingContext),
        audience: ActivityAudienceView::User,
    };

    let error = coordinator
        .start(command)
        .expect_err("mismatched step identity must fail");
    assert!(error.to_string().contains("Step 不匹配"));
}

#[test]
fn snapshot_is_parent_before_child_and_then_start_order() {
    let (coordinator, _) = coordinator();
    let root_id = coordinator.start(start_tool()).expect("start root");
    let child_step_id = sdk::RunStepId::new_v7();
    let child_id = coordinator
        .start(StartActivity {
            run_step_id: Some(child_step_id.clone()),
            parent_activity_id: Some(root_id.clone()),
            source: ActivitySource::RunStep(child_step_id),
            kind: ActivityKind::RunPhase(RunPhaseKind::PreparingContext),
            detail: ActivityDetail::Phase(RunPhaseKind::PreparingContext),
            audience: ActivityAudienceView::User,
        })
        .expect("start child");

    let snapshot = coordinator.snapshot();
    assert_eq!(snapshot.run_id, RunId::new("run-1"));
    assert_eq!(snapshot.revision, 2);
    assert_eq!(snapshot.activities[0].id, root_id);
    assert_eq!(snapshot.activities[1].id, child_id);
}

#[test]
fn phase_activity_keeps_explicit_kind_and_detail() {
    let (coordinator, _) = coordinator();
    let phase = StartActivity {
        run_step_id: None,
        parent_activity_id: None,
        source: ActivitySource::Run,
        kind: ActivityKind::RunPhase(RunPhaseKind::PreparingContext),
        detail: ActivityDetail::Phase(RunPhaseKind::PreparingContext),
        audience: ActivityAudienceView::User,
    };
    let activity_id = coordinator.start(phase).expect("start phase");
    let observation = coordinator
        .snapshot()
        .activities
        .into_iter()
        .find(|activity| activity.id == activity_id)
        .expect("phase activity");

    assert_eq!(
        observation.kind,
        sdk::ActivityKindView::RunPhase(sdk::RunPhaseKindView::PreparingContext)
    );
}
