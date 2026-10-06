use super::*;
use context::SessionId;
use std::time::Duration;

use sdk::{RunId, RunStepId};

fn identity() -> ToolCallIdentityData {
    ToolCallIdentityData {
        session_id: SessionId::new("session-1"),
        run_id: RunId::new("run-1"),
        step_id: RunStepId::new("step-1"),
        runtime_call_id: "runtime-call-1".to_string(),
        provider_call_id: Some("provider-call-1".to_string()),
        tool_name: "Bash".to_string(),
        call_index: 0,
        agent: false,
    }
}

fn backgrounded_state() -> BackgroundTaskState {
    BackgroundTaskState::Backgrounded {
        backgrounded_at: SystemTime::now(),
        deadline_snapshot: Some(SystemTime::now() + Duration::from_secs(3600)),
    }
}

#[test]
fn dispatch_creates_foreground_waiting_record() {
    let record = BackgroundTaskRecord::dispatch(identity(), "command=cargo test");
    assert!(matches!(
        record.state,
        BackgroundTaskState::ForegroundWaiting
    ));
    assert_eq!(record.identity.tool_name, "Bash");
    assert_eq!(record.invocation_summary, "command=cargo test");
    assert!(!record.is_terminal());
    assert!(!record.is_backgrounded());
}

#[test]
fn foreground_waiting_advances_to_backgrounded_capturing_deadline() {
    let record = BackgroundTaskRecord::dispatch(identity(), "command=build");
    let deadline = SystemTime::now() + Duration::from_secs(600);
    let advanced = record
        .advance(BackgroundTaskState::Backgrounded {
            backgrounded_at: SystemTime::now(),
            deadline_snapshot: Some(deadline),
        })
        .expect("ForegroundWaiting -> Backgrounded 应合法")
        .record;

    assert!(advanced.is_backgrounded());
    match advanced.state {
        BackgroundTaskState::Backgrounded {
            deadline_snapshot, ..
        } => assert_eq!(deadline_snapshot, Some(deadline)),
        other => panic!("应为 Backgrounded，实际 {other:?}"),
    }
}

#[test]
fn foreground_waiting_advances_directly_to_terminal_on_fast_path() {
    let record = BackgroundTaskRecord::dispatch(identity(), "pattern=**/*.rs");
    let advanced = record
        .advance(BackgroundTaskState::Terminal(
            BackgroundTaskTerminalKind::Success,
        ))
        .expect("快路径应允许直接进入终态")
        .record;

    assert!(advanced.is_terminal());
    assert_eq!(
        advanced.terminal_kind(),
        Some(BackgroundTaskTerminalKind::Success)
    );
}

#[test]
fn backgrounded_advances_to_terminal_kinds() {
    for kind in [
        BackgroundTaskTerminalKind::Success,
        BackgroundTaskTerminalKind::Failure,
        BackgroundTaskTerminalKind::TimedOut,
        BackgroundTaskTerminalKind::Stopped,
        BackgroundTaskTerminalKind::Invalidated {
            reason: BackgroundInvalidationReason::ProcessExit,
        },
    ] {
        let record = BackgroundTaskRecord::dispatch(identity(), "command=long")
            .advance(backgrounded_state())
            .unwrap()
            .record;
        let advanced = record
            .advance(BackgroundTaskState::Terminal(kind.clone()))
            .unwrap_or_else(|_| panic!("{kind:?} 应为合法后台终态"))
            .record;
        assert_eq!(advanced.terminal_kind(), Some(kind));
    }
}

#[test]
fn transition_rejects_terminal_regression() {
    let terminal = BackgroundTaskRecord::dispatch(identity(), "command=long")
        .advance(BackgroundTaskState::Terminal(
            BackgroundTaskTerminalKind::Success,
        ))
        .unwrap()
        .record;

    assert!(matches!(
        terminal.clone().advance(backgrounded_state()),
        Err(BackgroundTaskTransitionError::TerminalConflict { .. })
    ));
    assert!(matches!(
        terminal.advance(BackgroundTaskState::ForegroundWaiting),
        Err(BackgroundTaskTransitionError::TerminalConflict { .. })
    ));
}

#[test]
fn backgrounded_cannot_return_to_foreground() {
    let backgrounded = BackgroundTaskRecord::dispatch(identity(), "command=long")
        .advance(backgrounded_state())
        .unwrap()
        .record;

    assert!(matches!(
        backgrounded.advance(BackgroundTaskState::ForegroundWaiting),
        Err(BackgroundTaskTransitionError::InvalidTransition { .. })
    ));
}

#[test]
fn repeated_same_state_mutation_is_idempotent() {
    let waiting = BackgroundTaskRecord::dispatch(identity(), "command=long");
    let repeated = waiting
        .clone()
        .advance(BackgroundTaskState::ForegroundWaiting)
        .expect("相同状态应幂等");
    assert!(!repeated.changed);

    let backgrounded = waiting.advance(backgrounded_state()).unwrap().record;
    let repeated = backgrounded
        .advance(backgrounded_state())
        .expect("Backgrounded 重复应幂等");
    assert!(!repeated.changed);
}

#[test]
fn task_id_is_stable_and_unique_per_dispatch() {
    let first = BackgroundTaskRecord::dispatch(identity(), "a");
    let second = BackgroundTaskRecord::dispatch(identity(), "a");
    assert_ne!(first.task_id.as_str(), second.task_id.as_str());
}
