use sdk::{CancelRunStepOutcome, ControlDeadline, RunTerminationReason, TerminateRunOutcome};

fn round_trip<T>(value: &T) -> T
where
    T: serde::Serialize + serde::de::DeserializeOwned,
{
    serde_json::from_str(&serde_json::to_string(value).unwrap()).unwrap()
}

#[test]
fn cancel_run_step_outcomes_round_trip() {
    for outcome in [
        CancelRunStepOutcome::Accepted,
        CancelRunStepOutcome::AlreadyCancelling,
        CancelRunStepOutcome::NoActiveStep,
        CancelRunStepOutcome::RunTerminating,
        CancelRunStepOutcome::RunTerminal,
        CancelRunStepOutcome::NotFound,
    ] {
        assert_eq!(round_trip(&outcome), outcome);
    }
}

#[test]
fn terminate_run_outcomes_round_trip() {
    for outcome in [
        TerminateRunOutcome::Accepted,
        TerminateRunOutcome::AlreadyTerminating,
        TerminateRunOutcome::AlreadyTerminal,
        TerminateRunOutcome::NotFound,
    ] {
        assert_eq!(round_trip(&outcome), outcome);
    }
}

#[test]
fn termination_reasons_round_trip() {
    for reason in [
        RunTerminationReason::UserExit,
        RunTerminationReason::DoubleCtrlC,
        RunTerminationReason::QuitCommand,
        RunTerminationReason::ProcessSignal,
        RunTerminationReason::SessionShutdown,
        RunTerminationReason::ParentStepCancelled,
    ] {
        assert_eq!(round_trip(&reason), reason);
    }
}

#[test]
fn absolute_deadline_round_trips_without_runtime_handles() {
    let deadline = ControlDeadline::from_unix_millis(1_725_000_000_123);
    assert_eq!(round_trip(&deadline), deadline);
    assert_eq!(deadline.unix_millis(), 1_725_000_000_123);
}
