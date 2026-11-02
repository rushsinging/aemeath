use super::*;
use crate::domain::background_process::{
    BackgroundInvalidationReason, BackgroundProcessState, BackgroundProcessTerminalKind,
};
use context::ToolCallIdentityData;
use sdk::{RunId, RunStepId};
use share::ids::BackgroundProcessId;
use share::ids::SessionId;
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
    let task_id = supervisor.register(identity("1"), "command=cargo test", SystemTime::now());

    let snapshot = supervisor.snapshot(&task_id).expect("登记后应可查询");
    assert!(matches!(
        snapshot.state,
        BackgroundProcessState::ForegroundWaiting
    ));
}

#[test]
fn mark_backgrounded_advances_state_with_deadline_snapshot() {
    let supervisor = BackgroundProcessSupervisor::new();
    let task_id = supervisor.register(identity("1"), "command=build", SystemTime::now());
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
    let task_id = supervisor.register(identity("1"), "command=test", SystemTime::now());

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
    // 无文件任务回退终态文本（#1890）。
    let read = supervisor.read_task_log(&task_id, None, 1024).unwrap();
    assert_eq!(read.text, "3 passed");
    assert_eq!(read.cursor, read.total_written);
    assert_eq!(read.start_offset, 0);
    assert!(read.log_file.is_none());
}

#[test]
fn finish_after_backgrounded_records_terminal() {
    let supervisor = BackgroundProcessSupervisor::new();
    let task_id = supervisor.register(identity("1"), "command=long", SystemTime::now());
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
    let task_id = supervisor.register(identity("1"), "command=test", SystemTime::now());
    supervisor
        .finish(&task_id, BackgroundProcessTerminalKind::Success, None)
        .unwrap();
    let changed_again = supervisor
        .finish(&task_id, BackgroundProcessTerminalKind::Success, None)
        .expect("重复终态应幂等而非报错");
    assert!(!changed_again);
}

#[test]
fn direct_log_read_task_log_uses_file_as_source_of_truth() {
    let base = std::env::temp_dir().join(format!(
        "bgp-sup-tests-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let supervisor = BackgroundProcessSupervisor::new();
    let process_id = share::ids::BackgroundProcessId::new_v7();
    let (log, mut stdout, _stderr) =
        crate::application::background_process::log_file::TaskLogFile::open(
            &base,
            "sess-1",
            &process_id,
        )
        .unwrap();
    use std::io::Write as _;
    stdout.write_all(b"first-line\n").unwrap();
    drop(stdout);
    supervisor.register_direct(
        process_id.clone(),
        log.path().to_path_buf(),
        identity("1"),
        "command=watch",
        tokio_util::sync::CancellationToken::new(),
        SystemTime::now(),
    );

    // 尾部视图 + 增量游标：文件真相源，多次读取幂等。
    let read = supervisor.read_task_log(&process_id, None, 1024).unwrap();
    let (text, cursor, total) = (read.text, read.cursor, read.total_written);
    assert_eq!(text, "first-line\n");
    assert_eq!(cursor, total);
    assert_eq!(read.start_offset, 0);
    assert_eq!(
        read.log_file.as_deref(),
        Some(log.path()),
        "直绑任务必须回传日志文件路径"
    );
    let again = supervisor.read_task_log(&process_id, None, 1024).unwrap();
    assert_eq!(again.text, text);

    // 文件增长（子进程直写）→ 游标增量读出新段。
    let mut append = std::fs::OpenOptions::new()
        .append(true)
        .open(log.path())
        .unwrap();
    append.write_all(b"second-line\n").unwrap();
    let delta = supervisor
        .read_task_log(&process_id, Some(cursor), 1024)
        .unwrap();
    assert_eq!(delta.text, "second-line\n");
    assert_eq!(delta.cursor, delta.total_written);
    assert_eq!(delta.start_offset, cursor);
    assert!(delta.total_written > total);

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn snapshots_list_all_registered_tasks() {
    let supervisor = BackgroundProcessSupervisor::new();
    supervisor.register(identity("1"), "command=a", SystemTime::now());
    supervisor.register(identity("2"), "command=b", SystemTime::now());

    let snapshots = supervisor.snapshots();
    assert_eq!(snapshots.len(), 2);
    assert!(snapshots.iter().all(|record| !record.is_terminal()));
}

#[test]
fn peek_keeps_items_until_injection_confirmed() {
    let supervisor = BackgroundProcessSupervisor::new();
    let task_id = supervisor.register(identity("1"), "command=test", SystemTime::now());

    supervisor
        .finish(
            &task_id,
            BackgroundProcessTerminalKind::Success,
            Some("3 passed".to_string()),
        )
        .expect("终态推进");

    let items = supervisor.peek_unnotified_terminal_items();
    assert_eq!(items.len(), 1, "终态后应有一条待通知条目");
    assert_eq!(items[0].task_id, task_id.as_str());
    assert_eq!(items[0].tool_name, "Bash");
    assert!(matches!(
        items[0].status,
        context::BackgroundProcessCompletionStatus::Succeeded
    ));
    // 通知瘦身为 id + 状态（详情走 Logs 工具）；不再携带输出尾部。

    // peek 语义（注入确认制）：确认前重复可见（Run 收口后下个 Run
    // 仍可补注入）；确认后不再出现。
    assert_eq!(
        supervisor.peek_unnotified_terminal_items().len(),
        1,
        "确认前事实保留在监督器"
    );
    supervisor.mark_notified(&[task_id.as_str().to_string()]);
    assert!(
        supervisor.peek_unnotified_terminal_items().is_empty(),
        "注入确认后不再重复注入"
    );

    // #2146 方案 A 收口回滚：Run 收口时事实仍未注入 → 撤销交付标记，
    // 下个 Run 重新 peek 补注入（#252 不得静默丢失）。
    supervisor.mark_unnotified(&[task_id.as_str().to_string()]);
    assert_eq!(
        supervisor.peek_unnotified_terminal_items().len(),
        1,
        "回滚后事实重新可见"
    );
}

#[test]
fn peek_skips_invalidated_and_keeps_order() {
    let supervisor = BackgroundProcessSupervisor::new();
    let first = supervisor.register(identity("1"), "command=a", SystemTime::now());
    let second = supervisor.register(identity("2"), "command=b", SystemTime::now());
    let third = supervisor.register(identity("3"), "command=c", SystemTime::now());

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

    let items = supervisor.peek_unnotified_terminal_items();
    // Invalidated 是生命周期失效（resume 场景走失效投影），不产生 LLM 通知。
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].task_id, first.as_str(), "按终态顺序");
    assert_eq!(items[1].task_id, third.as_str());
    assert!(matches!(
        items[1].status,
        context::BackgroundProcessCompletionStatus::Cancelled
    ));
}

fn registered_backgrounded_task(
    supervisor: &BackgroundProcessSupervisor,
    token: tokio_util::sync::CancellationToken,
) -> BackgroundProcessId {
    let task_id = supervisor.register_with_cancellation(
        identity("stop-1"),
        "command=stop",
        token,
        SystemTime::now(),
    );
    supervisor.mark_backgrounded(&task_id, None).unwrap();
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
    let base = std::env::temp_dir().join(format!(
        "bgp-sup-incr-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let supervisor = BackgroundProcessSupervisor::new();
    let process_id = share::ids::BackgroundProcessId::new_v7();
    let (log, mut stdout, _stderr) =
        crate::application::background_process::log_file::TaskLogFile::open(
            &base,
            "sess-1",
            &process_id,
        )
        .unwrap();
    use std::io::Write as _;
    stdout.write_all(b"line-1\nline-2\n").unwrap();
    drop(stdout);
    let task_id = supervisor.register_direct(
        process_id,
        log.path().to_path_buf(),
        identity("1"),
        "command=watch",
        tokio_util::sync::CancellationToken::new(),
        SystemTime::now(),
    );

    // 尾部读取（无游标）：最近字节 + 读后游标。
    let tail = supervisor
        .read_task_log(&task_id, None, 4096)
        .expect("日志读取成功");
    assert!(tail.text.contains("line-1"));
    assert!(tail.text.contains("line-2"));
    assert!(tail.cursor > 0, "游标应推进到已读位置");
    let cursor = tail.cursor;

    // 增量读取（携带游标）：只返回新增字节（子进程直写文件）。
    let mut append = std::fs::OpenOptions::new()
        .append(true)
        .open(log.path())
        .unwrap();
    append.write_all(b"line-3\n").unwrap();
    let delta = supervisor
        .read_task_log(&task_id, Some(cursor), 4096)
        .expect("增量读取成功");
    assert_eq!(delta.text, "line-3\n", "增量只含新字节");
    assert!(delta.cursor > cursor);
    let next_cursor = delta.cursor;

    // 游标追平后读取：空增量。
    let empty = supervisor
        .read_task_log(&task_id, Some(next_cursor), 4096)
        .expect("游标追平后读取成功");
    assert!(empty.text.is_empty());

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn read_task_log_falls_back_to_terminal_output_when_no_stream() {
    let supervisor = BackgroundProcessSupervisor::new();
    let task_id = supervisor.register(identity("1"), "command=once", SystemTime::now());
    supervisor
        .finish(
            &task_id,
            BackgroundProcessTerminalKind::Success,
            Some("final output".to_string()),
        )
        .unwrap();

    let read = supervisor
        .read_task_log(&task_id, None, 4096)
        .expect("终态文本兜底");
    assert_eq!(read.text, "final output");
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
    let task_id = supervisor.register(
        identity("freeze"),
        "tool=Bash input=long",
        SystemTime::now(),
    );
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

// ── 注册携带派发时刻（时长自派发起算） ───────────────────────────────

#[test]
fn register_persists_caller_supplied_started_at() {
    let supervisor = BackgroundProcessSupervisor::new();
    let dispatch_time = SystemTime::now() - std::time::Duration::from_secs(12);
    let task_id = supervisor.register(identity("started-at"), "command=long", dispatch_time);

    let snapshot = supervisor.snapshot(&task_id).expect("任务存在");
    assert_eq!(
        snapshot.started_at, dispatch_time,
        "登记时固化调用方传入的派发时刻（覆盖前台等待段）"
    );
}

// ── 注入确认制（#252：完成事实不再随 Run 收口静默丢失） ─────────────

#[test]
fn unconfirmed_fact_survives_run_close_and_next_run_re_peeks() {
    let supervisor = BackgroundProcessSupervisor::new();
    let task_id = supervisor.register(identity("confirm"), "command=long", SystemTime::now());
    supervisor
        .finish(&task_id, BackgroundProcessTerminalKind::Success, None)
        .unwrap();

    // 第一个 Run：peek（入 reminder 队列）但 Run 收口、未注入确认。
    let first = supervisor.peek_unnotified_terminal_items();
    assert_eq!(first.len(), 1);

    // 第二个 Run（wakeup / 用户输入）：事实仍在，可再次 peek 补注入。
    let second = supervisor.peek_unnotified_terminal_items();
    assert_eq!(
        second.len(),
        1,
        "未确认事实必须存活到下一个 Run（修复：take 即标记曾静默丢失）"
    );

    // 注入确认后事实关闭。
    supervisor.mark_notified(&[task_id.as_str().to_string()]);
    assert!(supervisor.peek_unnotified_terminal_items().is_empty());
}

// ── #1962 按 runtime_call_id 追加过程行 ──

/// 独立临时日志文件（目录先建好，文件可不存在——append 会创建）。
fn temp_activity_log(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "bgp-activity-sup-{}-{}-{tag}",
        std::process::id(),
        SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).expect("建临时日志目录");
    dir.join("task.log")
}

#[test]
fn append_activity_line_writes_indirect_task_log() {
    let log_path = temp_activity_log("indirect");
    let supervisor = BackgroundProcessSupervisor::new();
    supervisor.register_indirect_log(
        share::ids::BackgroundProcessId::new_v7(),
        log_path.clone(),
        identity("a"),
        "Agent run",
        tokio_util::sync::CancellationToken::new(),
        SystemTime::now(),
    );

    let appended = supervisor.append_activity_line_for_runtime_call(
        "runtime-call-a",
        "started role=engineer model=claude-test",
    );

    assert!(appended, "间接日志任务应接受过程行");
    assert_eq!(
        std::fs::read_to_string(&log_path).unwrap(),
        "started role=engineer model=claude-test\n"
    );

    let _ = std::fs::remove_file(&log_path);
}

#[test]
fn append_activity_line_rejects_direct_log_task() {
    let log_path = temp_activity_log("direct");
    std::fs::write(&log_path, "direct-stdout\n").expect("预写直绑内容");
    let supervisor = BackgroundProcessSupervisor::new();
    supervisor.register_direct(
        share::ids::BackgroundProcessId::new_v7(),
        log_path.clone(),
        identity("b"),
        "Bash run",
        tokio_util::sync::CancellationToken::new(),
        SystemTime::now(),
    );

    let appended =
        supervisor.append_activity_line_for_runtime_call("runtime-call-b", "tool_call: Bash");

    assert!(!appended, "直绑任务禁止过程双写");
    assert_eq!(
        std::fs::read_to_string(&log_path).unwrap(),
        "direct-stdout\n",
        "直绑文件内容不得被过程行改动"
    );

    let _ = std::fs::remove_file(&log_path);
}

#[test]
fn append_activity_lines_then_terminal_summary_keep_order_once() {
    let log_path = temp_activity_log("terminal");
    let supervisor = BackgroundProcessSupervisor::new();
    supervisor.register_indirect_log(
        share::ids::BackgroundProcessId::new_v7(),
        log_path.clone(),
        identity("c"),
        "Agent run",
        tokio_util::sync::CancellationToken::new(),
        SystemTime::now(),
    );

    assert!(supervisor
        .append_activity_line_for_runtime_call("runtime-call-c", "started model=claude-test"));
    assert!(
        supervisor.append_activity_line_for_runtime_call("runtime-call-c", "tool_call: Bash, Read")
    );

    // 终态总结仍走唯一落盘通道 append_terminal。
    crate::application::background_process::log_file::TaskLogFile::from_path(log_path.clone())
        .append_terminal("final summary\n")
        .expect("终态总结追加");

    let text = std::fs::read_to_string(&log_path).unwrap();
    assert_eq!(
        text, "started model=claude-test\ntool_call: Bash, Read\nfinal summary\n",
        "过程行在前、总结在后"
    );
    assert_eq!(
        text.matches("final summary").count(),
        1,
        "终态总结只能出现一次"
    );

    let _ = std::fs::remove_file(&log_path);
}

#[test]
fn append_activity_line_unknown_call_id_returns_false() {
    let supervisor = BackgroundProcessSupervisor::new();
    assert!(
        !supervisor.append_activity_line_for_runtime_call("missing-call", "started model=x"),
        "无匹配任务应安静失败"
    );
}
