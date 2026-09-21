use super::RunActivityState;
use crate::tui::model::conversation::interaction::UiRunId;
use std::time::{Duration, Instant};

#[test]
fn root_revision_change_does_not_rebase_current_phase_clock() {
    let now = Instant::now();
    let run_id = UiRunId::from("main-1");
    let mut state = RunActivityState::default();
    state.sync_main_run(Some(&run_id), 1, 12_000, 10, 2_000, now);

    state.sync_main_run(
        Some(&run_id),
        2,
        14_000,
        10,
        2_000,
        now + Duration::from_secs(2),
    );

    assert_eq!(state.total_elapsed_secs(now + Duration::from_secs(3)), 15);
    assert_eq!(state.phase_elapsed_secs(now + Duration::from_secs(3)), 5);
}

#[test]
fn repeated_independent_timing_sync_does_not_reset_either_clock() {
    let now = Instant::now();
    let run_id = UiRunId::from("main-1");
    let mut state = RunActivityState::default();
    state.sync_main_run(Some(&run_id), 1, 12_000, 10, 2_000, now);

    state.sync_main_run(
        Some(&run_id),
        1,
        12_000,
        10,
        2_000,
        now + Duration::from_secs(2),
    );

    assert_eq!(state.total_elapsed_secs(now + Duration::from_secs(3)), 15);
    assert_eq!(state.phase_elapsed_secs(now + Duration::from_secs(3)), 5);
}

#[test]
fn phase_revision_change_does_not_rebase_root_total_clock() {
    let now = Instant::now();
    let run_id = UiRunId::from("main-1");
    let mut state = RunActivityState::default();
    state.sync_main_run(Some(&run_id), 1, 12_000, 10, 2_000, now);

    state.sync_main_run(
        Some(&run_id),
        1,
        12_000,
        11,
        0,
        now + Duration::from_secs(2),
    );

    assert_eq!(state.total_elapsed_secs(now + Duration::from_secs(3)), 15);
    assert_eq!(state.phase_elapsed_secs(now + Duration::from_secs(3)), 1);
}

#[test]
fn runtime_timing_baseline_advances_between_status_events() {
    let now = Instant::now();
    let run_id = UiRunId::from("main-1");
    let mut state = RunActivityState::default();
    state.sync_main_run(Some(&run_id), 1, 12_345, 1, 678, now);

    assert_eq!(
        state.total_elapsed_secs(now + Duration::from_millis(2_000)),
        14
    );
    assert_eq!(
        state.phase_elapsed_secs(now + Duration::from_millis(2_000)),
        2
    );
}

#[test]
fn repeated_render_sync_does_not_reset_runtime_timing_baseline() {
    let now = Instant::now();
    let run_id = UiRunId::from("main-1");
    let mut state = RunActivityState::default();
    state.sync_main_run(Some(&run_id), 1, 12_345, 1, 678, now);

    state.sync_main_run(
        Some(&run_id),
        1,
        12_345,
        1,
        678,
        now + Duration::from_secs(2),
    );

    assert_eq!(state.total_elapsed_secs(now + Duration::from_secs(3)), 15);
    assert_eq!(state.phase_elapsed_secs(now + Duration::from_secs(3)), 3);
}

#[test]
fn animation_frame_advances_while_activity_is_active() {
    let now = Instant::now();
    let run_id = UiRunId::from("main-1");
    let mut state = RunActivityState::default();
    state.sync_main_run(Some(&run_id), 1, 0, 1, 0, now);

    state.advance_frame();
    state.advance_frame();

    assert!(state.is_active());
    assert_eq!(state.frame, 2);
}
