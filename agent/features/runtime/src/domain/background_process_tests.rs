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

fn backgrounded_state() -> BackgroundProcessState {
    BackgroundProcessState::Backgrounded {
        backgrounded_at: SystemTime::now(),
        deadline_snapshot: Some(SystemTime::now() + Duration::from_secs(3600)),
    }
}

#[test]
fn dispatch_creates_foreground_waiting_record() {
    let record = BackgroundProcessRecord::dispatch(identity(), "command=cargo test");
    assert!(matches!(
        record.state,
        BackgroundProcessState::ForegroundWaiting
    ));
    assert_eq!(record.identity.tool_name, "Bash");
    assert_eq!(record.invocation_summary, "command=cargo test");
    assert!(!record.is_terminal());
    assert!(!record.is_backgrounded());
}

#[test]
fn foreground_waiting_advances_to_backgrounded_capturing_deadline() {
    let record = BackgroundProcessRecord::dispatch(identity(), "command=build");
    let deadline = SystemTime::now() + Duration::from_secs(600);
    let advanced = record
        .advance(BackgroundProcessState::Backgrounded {
            backgrounded_at: SystemTime::now(),
            deadline_snapshot: Some(deadline),
        })
        .expect("ForegroundWaiting -> Backgrounded 应合法")
        .record;

    assert!(advanced.is_backgrounded());
    match advanced.state {
        BackgroundProcessState::Backgrounded {
            deadline_snapshot, ..
        } => assert_eq!(deadline_snapshot, Some(deadline)),
        other => panic!("应为 Backgrounded，实际 {other:?}"),
    }
}

#[test]
fn foreground_waiting_advances_directly_to_terminal_on_fast_path() {
    let record = BackgroundProcessRecord::dispatch(identity(), "pattern=**/*.rs");
    let advanced = record
        .advance(BackgroundProcessState::Terminal(
            BackgroundProcessTerminalKind::Success,
        ))
        .expect("快路径应允许直接进入终态")
        .record;

    assert!(advanced.is_terminal());
    assert_eq!(
        advanced.terminal_kind(),
        Some(BackgroundProcessTerminalKind::Success)
    );
}

#[test]
fn backgrounded_advances_to_terminal_kinds() {
    for kind in [
        BackgroundProcessTerminalKind::Success,
        BackgroundProcessTerminalKind::Failure,
        BackgroundProcessTerminalKind::TimedOut,
        BackgroundProcessTerminalKind::Stopped,
        BackgroundProcessTerminalKind::Invalidated {
            reason: BackgroundInvalidationReason::ProcessExit,
        },
    ] {
        let record = BackgroundProcessRecord::dispatch(identity(), "command=long")
            .advance(backgrounded_state())
            .unwrap()
            .record;
        let advanced = record
            .advance(BackgroundProcessState::Terminal(kind.clone()))
            .unwrap_or_else(|_| panic!("{kind:?} 应为合法后台终态"))
            .record;
        assert_eq!(advanced.terminal_kind(), Some(kind));
    }
}

#[test]
fn transition_rejects_terminal_regression() {
    let terminal = BackgroundProcessRecord::dispatch(identity(), "command=long")
        .advance(BackgroundProcessState::Terminal(
            BackgroundProcessTerminalKind::Success,
        ))
        .unwrap()
        .record;

    assert!(matches!(
        terminal.clone().advance(backgrounded_state()),
        Err(BackgroundProcessTransitionError::TerminalConflict { .. })
    ));
    assert!(matches!(
        terminal.advance(BackgroundProcessState::ForegroundWaiting),
        Err(BackgroundProcessTransitionError::TerminalConflict { .. })
    ));
}

#[test]
fn backgrounded_cannot_return_to_foreground() {
    let backgrounded = BackgroundProcessRecord::dispatch(identity(), "command=long")
        .advance(backgrounded_state())
        .unwrap()
        .record;

    assert!(matches!(
        backgrounded.advance(BackgroundProcessState::ForegroundWaiting),
        Err(BackgroundProcessTransitionError::InvalidTransition { .. })
    ));
}

#[test]
fn repeated_same_state_mutation_is_idempotent() {
    let waiting = BackgroundProcessRecord::dispatch(identity(), "command=long");
    let repeated = waiting
        .clone()
        .advance(BackgroundProcessState::ForegroundWaiting)
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
    let first = BackgroundProcessRecord::dispatch(identity(), "a");
    let second = BackgroundProcessRecord::dispatch(identity(), "a");
    assert_ne!(first.task_id.as_str(), second.task_id.as_str());
}

// ── 终态完成时刻冻结（时长不再随查询时刻增长） ────────────────────────

#[test]
fn mark_finished_records_terminal_time_once() {
    let terminal = BackgroundProcessRecord::dispatch(identity(), "command=long")
        .advance(BackgroundProcessState::Terminal(
            BackgroundProcessTerminalKind::Success,
        ))
        .unwrap()
        .record;
    let mut record = terminal;

    let first_mark = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
    record.mark_finished(first_mark);
    assert_eq!(record.finished_at, Some(first_mark));

    // 重复标记不覆盖首次完成时刻（幂等）。
    let second_mark = SystemTime::UNIX_EPOCH + Duration::from_secs(2_000_000);
    record.mark_finished(second_mark);
    assert_eq!(record.finished_at, Some(first_mark));
}

#[test]
fn mark_finished_is_ignored_on_non_terminal_record() {
    let mut record = BackgroundProcessRecord::dispatch(identity(), "command=long");
    record.mark_finished(SystemTime::UNIX_EPOCH + Duration::from_secs(1));
    assert!(record.finished_at.is_none(), "非终态记录不得固化完成时刻");
}

#[test]
fn record_deserializes_without_finished_at_for_legacy_snapshot() {
    let terminal = BackgroundProcessRecord::dispatch(identity(), "command=legacy")
        .advance(BackgroundProcessState::Terminal(
            BackgroundProcessTerminalKind::Success,
        ))
        .unwrap()
        .record;
    // 模拟旧快照：剥离 finished_at 字段后应仍可反序列化（serde default）。
    let mut value = serde_json::to_value(&terminal).unwrap();
    value
        .as_object_mut()
        .expect("record 序列化为对象")
        .remove("finished_at");
    let restored: BackgroundProcessRecord = serde_json::from_value(value).expect("旧快照可恢复");
    assert!(restored.finished_at.is_none());
}
