use super::*;
use crate::domain::background_process::{
    BackgroundInvalidationReason, BackgroundProcessState, BackgroundProcessTerminalKind,
};
use context::{SessionId, ToolCallIdentityData};
use sdk::{RunId, RunStepId};
use share::ids::BackgroundProcessId;
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
    let supervisor = BackgroundProcessSupervisor::new();
    let task_id = supervisor.register(identity("1"), "command=cargo test");

    let snapshot = supervisor.snapshot(&task_id).expect("登记后应可查询");
    assert!(matches!(
        snapshot.state,
        BackgroundProcessState::ForegroundWaiting
    ));
}

#[test]
fn mark_backgrounded_advances_state_with_deadline_snapshot() {
    let supervisor = BackgroundProcessSupervisor::new();
    let task_id = supervisor.register(identity("1"), "command=build");
    let deadline = SystemTime::now() + Duration::from_secs(600);

    supervisor
        .mark_backgrounded(&task_id, Some(deadline))
        .expect("转后台应成功");

    let snapshot = supervisor.snapshot(&task_id).unwrap();
    match snapshot.state {
        BackgroundProcessState::Backgrounded {
            deadline_snapshot, ..
        } => assert_eq!(deadline_snapshot, Some(deadline)),
        other => panic!("应为 Backgrounded，实际 {other:?}"),
    }
}

#[test]
fn finish_records_terminal_kind_and_output() {
    let supervisor = BackgroundProcessSupervisor::new();
    let task_id = supervisor.register(identity("1"), "command=test");
    supervisor.record_output(&task_id, b"3 passed");

    let changed = supervisor
        .finish(
            &task_id,
            BackgroundProcessTerminalKind::Success,
            Some("3 passed".to_string()),
        )
        .expect("快路径完成应合法");

    assert!(changed);
    assert_eq!(
        supervisor.snapshot(&task_id).unwrap().terminal_kind(),
        Some(BackgroundProcessTerminalKind::Success)
    );
    let (text, _) = supervisor.read_output_tail(&task_id, 1024).unwrap();
    assert_eq!(text, "3 passed");
}

#[test]
fn finish_after_backgrounded_records_terminal() {
    let supervisor = BackgroundProcessSupervisor::new();
    let task_id = supervisor.register(identity("1"), "command=long");
    supervisor
        .mark_backgrounded(&task_id, None)
        .expect("转后台应成功");

    supervisor
        .finish(&task_id, BackgroundProcessTerminalKind::TimedOut, None)
        .expect("后台终态应合法");

    assert_eq!(
        supervisor.snapshot(&task_id).unwrap().terminal_kind(),
        Some(BackgroundProcessTerminalKind::TimedOut)
    );
}

#[test]
fn finish_unknown_task_errors() {
    let supervisor = BackgroundProcessSupervisor::new();
    let unknown = BackgroundProcessId::new_v7();
    assert!(supervisor
        .finish(&unknown, BackgroundProcessTerminalKind::Success, None)
        .is_err());
}

#[test]
fn repeated_finish_is_idempotent() {
    let supervisor = BackgroundProcessSupervisor::new();
    let task_id = supervisor.register(identity("1"), "command=test");
    supervisor
        .finish(&task_id, BackgroundProcessTerminalKind::Success, None)
        .unwrap();
    let changed_again = supervisor
        .finish(&task_id, BackgroundProcessTerminalKind::Success, None)
        .expect("重复终态应幂等而非报错");
    assert!(!changed_again);
}

#[test]
fn invalidate_all_marks_active_tasks_and_keeps_terminals() {
    let supervisor = BackgroundProcessSupervisor::new();
    let waiting = supervisor.register(identity("waiting"), "command=a");
    let backgrounded = supervisor.register(identity("bg"), "command=b");
    supervisor.mark_backgrounded(&backgrounded, None).unwrap();
    let finished = supervisor.register(identity("done"), "command=c");
    supervisor
        .finish(&finished, BackgroundProcessTerminalKind::Success, None)
        .unwrap();

    let invalidated_count = supervisor.invalidate_all(BackgroundInvalidationReason::ProcessExit);

    assert_eq!(invalidated_count, 2, "仅活跃任务失效");
    assert_eq!(
        supervisor.snapshot(&waiting).unwrap().terminal_kind(),
        Some(BackgroundProcessTerminalKind::Invalidated {
            reason: BackgroundInvalidationReason::ProcessExit
        })
    );
    assert_eq!(
        supervisor.snapshot(&backgrounded).unwrap().terminal_kind(),
        Some(BackgroundProcessTerminalKind::Invalidated {
            reason: BackgroundInvalidationReason::ProcessExit
        })
    );
    assert_eq!(
        supervisor.snapshot(&finished).unwrap().terminal_kind(),
        Some(BackgroundProcessTerminalKind::Success)
    );
}

#[test]
fn output_read_tail_is_non_consumptive_across_reads() {
    let supervisor = BackgroundProcessSupervisor::new();
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
    let supervisor = BackgroundProcessSupervisor::new();
    supervisor.register(identity("1"), "command=a");
    supervisor.register(identity("2"), "command=b");

    let snapshots = supervisor.snapshots();
    assert_eq!(snapshots.len(), 2);
    assert!(snapshots.iter().all(|record| !record.is_terminal()));
}

#[test]
fn take_unnotified_terminal_items_returns_each_completion_once() {
    let supervisor = BackgroundProcessSupervisor::new();
    let task_id = supervisor.register(identity("1"), "command=test");
    supervisor.record_output(&task_id, b"3 passed\n");

    supervisor
        .finish(&task_id, BackgroundProcessTerminalKind::Success, None)
        .expect("终态推进");

    let items = supervisor.take_unnotified_terminal_items();
    assert_eq!(items.len(), 1, "终态后应有一条待通知条目");
    assert_eq!(items[0].task_id, task_id.as_str());
    assert_eq!(items[0].tool_name, "Bash");
    assert!(matches!(
        items[0].status,
        context::BackgroundProcessCompletionStatus::Succeeded
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
    let supervisor = BackgroundProcessSupervisor::new();
    let first = supervisor.register(identity("1"), "command=a");
    let second = supervisor.register(identity("2"), "command=b");
    let third = supervisor.register(identity("3"), "command=c");

    supervisor
        .finish(&first, BackgroundProcessTerminalKind::Success, None)
        .unwrap();
    supervisor
        .finish(
            &second,
            BackgroundProcessTerminalKind::Invalidated {
                reason: BackgroundInvalidationReason::ProcessExit,
            },
            None,
        )
        .unwrap();
    supervisor
        .finish(&third, BackgroundProcessTerminalKind::Stopped, None)
        .unwrap();

    let items = supervisor.take_unnotified_terminal_items();
    // Invalidated 是生命周期失效（resume 场景走失效投影），不产生 LLM 通知。
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].task_id, first.as_str(), "按终态顺序");
    assert_eq!(items[1].task_id, third.as_str());
    assert!(matches!(
        items[1].status,
        context::BackgroundProcessCompletionStatus::Cancelled
    ));
}

#[test]
fn take_unnotified_terminal_items_caps_output_tail_bytes() {
    let supervisor = BackgroundProcessSupervisor::new();
    let task_id = supervisor.register(identity("1"), "command=verbose");
    supervisor.record_output(&task_id, vec![b'x'; 8192].as_slice());

    supervisor
        .finish(&task_id, BackgroundProcessTerminalKind::Success, None)
        .unwrap();

    let items = supervisor.take_unnotified_terminal_items();
    let tail_bytes = items[0].output_tail.len();
    assert!(
        tail_bytes <= crate::application::constants::BACKGROUND_PROCESS_NOTIFICATION_TAIL_BYTES,
        "通知尾部截断（实际 {tail_bytes} 字节）"
    );
}

// ── #252 PR3：stop 请求与日志游标读取 ───────────────────────────────

fn registered_backgrounded_task(
    supervisor: &BackgroundProcessSupervisor,
    token: tokio_util::sync::CancellationToken,
) -> BackgroundProcessId {
    let task_id = supervisor.register_with_cancellation(identity("1"), "command=build", token);
    supervisor
        .mark_backgrounded(&task_id, None)
        .expect("转后台成功");
    task_id
}

#[test]
fn stop_task_cancels_child_token_and_reports_current_state() {
    let supervisor = BackgroundProcessSupervisor::new();
    let token = tokio_util::sync::CancellationToken::new();
    let task_id = registered_backgrounded_task(&supervisor, token.clone());

    let outcome = supervisor.stop_task(&task_id).expect("stop 请求成功");
    assert!(
        token.is_cancelled(),
        "stop 必须取消子任务 cancellation token"
    );

    use crate::application::background_process::supervisor::StopRequestOutcome;
    match outcome {
        StopRequestOutcome::SignalSent { state } => {
            assert!(
                matches!(state, BackgroundProcessState::Backgrounded { .. }),
                "stop 时任务应仍在 Backgrounded（真实终态由执行体收口）"
            );
        }
        other => panic!("应返回 SignalSent，实际 {other:?}"),
    }
}

#[test]
fn stop_task_on_terminal_task_returns_terminal_without_side_effect() {
    let supervisor = BackgroundProcessSupervisor::new();
    let token = tokio_util::sync::CancellationToken::new();
    let task_id = registered_backgrounded_task(&supervisor, token.clone());
    supervisor
        .finish(&task_id, BackgroundProcessTerminalKind::Success, None)
        .unwrap();

    let outcome = supervisor.stop_task(&task_id).expect("stop 请求成功");
    match outcome {
        StopRequestOutcome::AlreadyTerminal(kind) => {
            assert_eq!(kind, BackgroundProcessTerminalKind::Success);
        }
        other => panic!("终态任务 stop 应幂等返回终态，实际 {other:?}"),
    }
}

#[test]
fn stop_task_unknown_id_reports_error() {
    let supervisor = BackgroundProcessSupervisor::new();
    let result = supervisor.stop_task(&BackgroundProcessId::new_v7());
    assert!(result.is_err(), "未知任务 stop 必须报错");
}

#[test]
fn read_task_log_returns_tail_then_incremental_delta() {
    let supervisor = BackgroundProcessSupervisor::new();
    let task_id = supervisor.register(identity("1"), "command=watch");
    supervisor.record_output(&task_id, b"line-1\nline-2\n");

    // 尾部读取（无游标）：最近字节 + 读后游标。
    let (text, cursor, _total) = supervisor
        .read_task_log(&task_id, None, 4096)
        .expect("日志读取成功");
    assert!(text.contains("line-1"));
    assert!(text.contains("line-2"));
    assert!(cursor > 0, "游标应推进到已读位置");

    // 增量读取（携带游标）：只返回新增字节。
    supervisor.record_output(&task_id, b"line-3\n");
    let (delta, next_cursor, _) = supervisor
        .read_task_log(&task_id, Some(cursor), 4096)
        .expect("增量读取成功");
    assert_eq!(delta, "line-3\n", "增量只含新字节");
    assert!(next_cursor > cursor);

    // 游标追平后读取：空增量。
    let (empty, _, _) = supervisor
        .read_task_log(&task_id, Some(next_cursor), 4096)
        .expect("游标追平后读取成功");
    assert!(empty.is_empty());
}

#[test]
fn read_task_log_falls_back_to_terminal_output_when_no_stream() {
    let supervisor = BackgroundProcessSupervisor::new();
    let task_id = supervisor.register(identity("1"), "command=once");
    supervisor
        .finish(
            &task_id,
            BackgroundProcessTerminalKind::Success,
            Some("final output".to_string()),
        )
        .unwrap();

    let (text, _cursor, _) = supervisor
        .read_task_log(&task_id, None, 4096)
        .expect("终态文本兜底");
    assert_eq!(text, "final output");
}

#[test]
fn read_task_log_unknown_id_returns_none() {
    let supervisor = BackgroundProcessSupervisor::new();
    assert!(supervisor
        .read_task_log(&BackgroundProcessId::new_v7(), None, 64)
        .is_none());
}

// ── 终态完成时刻冻结（时长不再随查询时刻增长） ────────────────────────

#[test]
fn finish_freezes_completion_time_on_record() {
    let supervisor = BackgroundProcessSupervisor::new();
    let task_id = supervisor.register(identity("freeze"), "tool=Bash input=long");
    supervisor.mark_backgrounded(&task_id, None).unwrap();

    supervisor
        .finish(&task_id, BackgroundProcessTerminalKind::Success, None)
        .unwrap();

    let snapshot = supervisor.snapshot(&task_id).expect("任务存在");
    assert!(snapshot.finished_at.is_some(), "首次终态推进应固化完成时刻");

    // 重复 finish 幂等，不刷新完成时刻。
    let frozen_at = snapshot.finished_at;
    std::thread::sleep(std::time::Duration::from_millis(5));
    supervisor
        .finish(&task_id, BackgroundProcessTerminalKind::Success, None)
        .unwrap();
    assert_eq!(
        supervisor.snapshot(&task_id).unwrap().finished_at,
        frozen_at,
        "重复终态推进不得刷新完成时刻"
    );
}

#[test]
fn invalidate_all_freezes_completion_time_on_active_tasks() {
    let supervisor = BackgroundProcessSupervisor::new();
    let task_id = supervisor.register(identity("invalidate"), "tool=Bash input=long");
    supervisor.mark_backgrounded(&task_id, None).unwrap();

    let invalidated = supervisor.invalidate_all(BackgroundInvalidationReason::ProcessExit);
    assert_eq!(invalidated, 1);
    let snapshot = supervisor.snapshot(&task_id).unwrap();
    assert!(snapshot.finished_at.is_some(), "失效收口同样固化完成时刻");
}
