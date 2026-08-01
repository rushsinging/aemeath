use super::RunActivityState;
use crate::tui::model::conversation::interaction::UiRunId;
use std::time::{Duration, Instant};

#[test]
fn runtime_timing_baseline_advances_between_status_events() {
    let now = Instant::now();
    let run_id = UiRunId::from("main-1");
    let mut state = RunActivityState::default();
    state.sync_main_run(Some(&run_id), true, 12_345, 678, now);

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
fn invoking_model_becomes_silent_at_ten_seconds() {
    let now = Instant::now();
    let run_id = UiRunId::from("main-1");
    let mut state = RunActivityState::default();
    state.sync_main_run(Some(&run_id), true, 0, 0, now);

    assert!(!state.is_model_silent(now + Duration::from_millis(9_999)));
    assert!(state.is_model_silent(now + Duration::from_secs(10)));
}

#[test]
fn model_activity_resets_silence_and_changes_interval_identity() {
    let now = Instant::now();
    let run_id = UiRunId::from("main-1");
    let mut state = RunActivityState::default();
    state.sync_main_run(Some(&run_id), true, 0, 0, now);
    let first_id = state.silence_block_id();

    assert!(state.observe_main_model_activity(&run_id, now + Duration::from_secs(8)));

    assert!(!state.is_model_silent(now + Duration::from_secs(17)));
    assert!(state.is_model_silent(now + Duration::from_secs(18)));
    assert_ne!(state.silence_block_id(), first_id);
}

#[test]
fn leaving_and_reentering_invoking_model_starts_fresh_interval() {
    let now = Instant::now();
    let run_id = UiRunId::from("main-1");
    let mut state = RunActivityState::default();
    state.sync_main_run(Some(&run_id), true, 0, 0, now);
    state.sync_main_run(
        Some(&run_id),
        false,
        2_000,
        2_000,
        now + Duration::from_secs(2),
    );
    assert!(!state.is_model_silent(now + Duration::from_secs(20)));

    state.sync_main_run(
        Some(&run_id),
        true,
        20_000,
        0,
        now + Duration::from_secs(20),
    );
    assert!(!state.is_model_silent(now + Duration::from_secs(29)));
    assert!(state.is_model_silent(now + Duration::from_secs(30)));
}

#[test]
fn sub_or_stale_run_activity_does_not_reset_main_silence() {
    let now = Instant::now();
    let main_run_id = UiRunId::from("main-1");
    let sub_run_id = UiRunId::from("sub-1");
    let mut state = RunActivityState::default();
    state.sync_main_run(Some(&main_run_id), true, 0, 0, now);

    assert!(!state.observe_main_model_activity(&sub_run_id, now + Duration::from_secs(9)));
    assert!(state.is_model_silent(now + Duration::from_secs(10)));
}

#[test]
fn animation_frame_advances_without_changing_identity() {
    let now = Instant::now();
    let run_id = UiRunId::from("main-1");
    let mut state = RunActivityState::default();
    state.sync_main_run(Some(&run_id), true, 0, 0, now);
    let block_id = state.silence_block_id();

    state.advance_frame();
    state.advance_frame();

    assert_eq!(state.frame, 2);
    assert_eq!(state.silence_block_id(), block_id);
}
