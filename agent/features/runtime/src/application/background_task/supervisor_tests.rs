use super::*;
use crate::domain::background_task::{
    BackgroundInvalidationReason, BackgroundTaskState, BackgroundTaskTerminalKind,
};
use context::{SessionId, ToolCallIdentityData};
use sdk::{RunId, RunStepId};
use share::ids::BackgroundTaskId;
use std::time::{Duration, SystemTime};

fn identity(call_suffix: &str) -> ToolCallIdentityData {
    ToolCallIdentityData {
        session_id: SessionId::new("session-1"),
        run_id: RunId::new("run-1"),
        step_id: RunStepId::new("step-1"),
        runtime_call_id: format!("runtime-call-{call_suffix}"),
        provider_call_id: None,
        tool_name: "Bash".to_string(),
        call_index: 0,
        agent: false,
    }
}

#[test]
fn register_creates_foreground_waiting_task() {
    let supervisor = BackgroundTaskSupervisor::new();
    let task_id = supervisor.register(identity("1"), "command=cargo test");

    let snapshot = supervisor.snapshot(&task_id).expect("登记后应可查询");
    assert!(matches!(
        snapshot.state,
        BackgroundTaskState::ForegroundWaiting
    ));
}

#[test]
fn mark_backgrounded_advances_state_with_deadline_snapshot() {
    let supervisor = BackgroundTaskSupervisor::new();
    let task_id = supervisor.register(identity("1"), "command=build");
    let deadline = SystemTime::now() + Duration::from_secs(600);

    supervisor
        .mark_backgrounded(&task_id, Some(deadline))
        .expect("转后台应成功");

    let snapshot = supervisor.snapshot(&task_id).unwrap();
    match snapshot.state {
        BackgroundTaskState::Backgrounded {
            deadline_snapshot, ..
        } => assert_eq!(deadline_snapshot, Some(deadline)),
        other => panic!("应为 Backgrounded，实际 {other:?}"),
    }
}

#[test]
fn finish_records_terminal_kind_and_output() {
    let supervisor = BackgroundTaskSupervisor::new();
    let task_id = supervisor.register(identity("1"), "command=test");
    supervisor.record_output(&task_id, b"3 passed");

    let changed = supervisor
        .finish(
            &task_id,
            BackgroundTaskTerminalKind::Success,
            Some("3 passed".to_string()),
        )
        .expect("快路径完成应合法");

    assert!(changed);
    assert_eq!(
        supervisor.snapshot(&task_id).unwrap().terminal_kind(),
        Some(BackgroundTaskTerminalKind::Success)
    );
    let (text, _) = supervisor.read_output_tail(&task_id, 1024).unwrap();
    assert_eq!(text, "3 passed");
}

#[test]
fn finish_after_backgrounded_records_terminal() {
    let supervisor = BackgroundTaskSupervisor::new();
    let task_id = supervisor.register(identity("1"), "command=long");
    supervisor
        .mark_backgrounded(&task_id, None)
        .expect("转后台应成功");

    supervisor
        .finish(&task_id, BackgroundTaskTerminalKind::TimedOut, None)
        .expect("后台终态应合法");

    assert_eq!(
        supervisor.snapshot(&task_id).unwrap().terminal_kind(),
        Some(BackgroundTaskTerminalKind::TimedOut)
    );
}

#[test]
fn finish_unknown_task_errors() {
    let supervisor = BackgroundTaskSupervisor::new();
    let unknown = BackgroundTaskId::new_v7();
    assert!(supervisor
        .finish(&unknown, BackgroundTaskTerminalKind::Success, None)
        .is_err());
}

#[test]
fn repeated_finish_is_idempotent() {
    let supervisor = BackgroundTaskSupervisor::new();
    let task_id = supervisor.register(identity("1"), "command=test");
    supervisor
        .finish(&task_id, BackgroundTaskTerminalKind::Success, None)
        .unwrap();
    let changed_again = supervisor
        .finish(&task_id, BackgroundTaskTerminalKind::Success, None)
        .expect("重复终态应幂等而非报错");
    assert!(!changed_again);
}

#[test]
fn invalidate_all_marks_active_tasks_and_keeps_terminals() {
    let supervisor = BackgroundTaskSupervisor::new();
    let waiting = supervisor.register(identity("waiting"), "command=a");
    let backgrounded = supervisor.register(identity("bg"), "command=b");
    supervisor.mark_backgrounded(&backgrounded, None).unwrap();
    let finished = supervisor.register(identity("done"), "command=c");
    supervisor
        .finish(&finished, BackgroundTaskTerminalKind::Success, None)
        .unwrap();

    let invalidated_count = supervisor.invalidate_all(BackgroundInvalidationReason::ProcessExit);

    assert_eq!(invalidated_count, 2, "仅活跃任务失效");
    assert_eq!(
        supervisor.snapshot(&waiting).unwrap().terminal_kind(),
        Some(BackgroundTaskTerminalKind::Invalidated {
            reason: BackgroundInvalidationReason::ProcessExit
        })
    );
    assert_eq!(
        supervisor.snapshot(&backgrounded).unwrap().terminal_kind(),
        Some(BackgroundTaskTerminalKind::Invalidated {
            reason: BackgroundInvalidationReason::ProcessExit
        })
    );
    assert_eq!(
        supervisor.snapshot(&finished).unwrap().terminal_kind(),
        Some(BackgroundTaskTerminalKind::Success)
    );
}

#[test]
fn output_read_tail_is_non_consumptive_across_reads() {
    let supervisor = BackgroundTaskSupervisor::new();
    let task_id = supervisor.register(identity("1"), "command=watch");
    supervisor.record_output(&task_id, b"first-line\n");

    let first = supervisor.read_output_tail(&task_id, 1024).unwrap();
    let second = supervisor.read_output_tail(&task_id, 1024).unwrap();
    assert_eq!(first, second);

    supervisor.record_output(&task_id, b"second-line\n");
    let (text, _) = supervisor.read_output_tail(&task_id, 1024).unwrap();
    assert_eq!(text, "first-line\nsecond-line\n");
}

#[test]
fn snapshots_list_all_registered_tasks() {
    let supervisor = BackgroundTaskSupervisor::new();
    supervisor.register(identity("1"), "command=a");
    supervisor.register(identity("2"), "command=b");

    let snapshots = supervisor.snapshots();
    assert_eq!(snapshots.len(), 2);
    assert!(snapshots.iter().all(|record| !record.is_terminal()));
}

#[test]
fn take_unnotified_terminal_items_returns_each_completion_once() {
    let supervisor = BackgroundTaskSupervisor::new();
    let task_id = supervisor.register(identity("1"), "command=test");
    supervisor.record_output(&task_id, b"3 passed\n");

    supervisor
        .finish(&task_id, BackgroundTaskTerminalKind::Success, None)
        .expect("终态推进");

    let items = supervisor.take_unnotified_terminal_items();
    assert_eq!(items.len(), 1, "终态后应有一条待通知条目");
    assert_eq!(items[0].task_id, task_id.as_str());
    assert_eq!(items[0].tool_name, "Bash");
    assert!(matches!(
        items[0].status,
        context::BackgroundTaskCompletionStatus::Succeeded
    ));
    assert!(items[0].output_tail.contains("3 passed"), "通知带输出尾部");

    // take 语义：再次调用为空（已通知不再重复注入）。
    assert!(
        supervisor.take_unnotified_terminal_items().is_empty(),
        "已通知条目不得重复"
    );
}

#[test]
fn take_unnotified_terminal_items_skips_invalidated_and_keeps_order() {
    let supervisor = BackgroundTaskSupervisor::new();
    let first = supervisor.register(identity("1"), "command=a");
    let second = supervisor.register(identity("2"), "command=b");
    let third = supervisor.register(identity("3"), "command=c");

    supervisor
        .finish(&first, BackgroundTaskTerminalKind::Success, None)
        .unwrap();
    supervisor
        .finish(
            &second,
            BackgroundTaskTerminalKind::Invalidated {
                reason: BackgroundInvalidationReason::ProcessExit,
            },
            None,
        )
        .unwrap();
    supervisor
        .finish(&third, BackgroundTaskTerminalKind::Stopped, None)
        .unwrap();

    let items = supervisor.take_unnotified_terminal_items();
    // Invalidated 是生命周期失效（resume 场景走失效投影），不产生 LLM 通知。
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].task_id, first.as_str(), "按终态顺序");
    assert_eq!(items[1].task_id, third.as_str());
    assert!(matches!(
        items[1].status,
        context::BackgroundTaskCompletionStatus::Cancelled
    ));
}

#[test]
fn take_unnotified_terminal_items_caps_output_tail_bytes() {
    let supervisor = BackgroundTaskSupervisor::new();
    let task_id = supervisor.register(identity("1"), "command=verbose");
    supervisor.record_output(&task_id, vec![b'x'; 8192].as_slice());

    supervisor
        .finish(&task_id, BackgroundTaskTerminalKind::Success, None)
        .unwrap();

    let items = supervisor.take_unnotified_terminal_items();
    let tail_bytes = items[0].output_tail.len();
    assert!(
        tail_bytes <= crate::application::constants::BACKGROUND_TASK_NOTIFICATION_TAIL_BYTES,
        "通知尾部截断（实际 {tail_bytes} 字节）"
    );
}
