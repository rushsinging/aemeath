use super::*;
use std::time::SystemTime;

#[test]
fn notify_route_targets_active_main_run_when_present() {
    let registry =
        std::sync::Arc::new(crate::application::run::active_registry::ActiveRunRegistry::default());
    let runtime = BackgroundProcessRuntime::for_test(registry.clone());

    // 无 active Run → wakeup 信号。
    assert!(matches!(
        runtime.notify_route(),
        BackgroundNotifyRoute::WakeupSignal
    ));

    // 有 active Main Run → reminder 事件路由到该 Run。
    let run_id = sdk::RunId::new_v7();
    registry.activate_main_for_test(run_id.clone());
    match runtime.notify_route() {
        BackgroundNotifyRoute::Reminder(target) => assert_eq!(target, run_id),
        other => panic!("应路由 reminder，实际 {other:?}"),
    }
}

#[test]
fn notify_route_clears_with_run_lifecycle() {
    let registry =
        std::sync::Arc::new(crate::application::run::active_registry::ActiveRunRegistry::default());
    let runtime = BackgroundProcessRuntime::for_test(registry.clone());
    let run_id = sdk::RunId::new_v7();
    registry.activate_main_for_test(run_id.clone());

    registry.clear_for_test(&run_id);
    assert!(
        matches!(runtime.notify_route(), BackgroundNotifyRoute::WakeupSignal),
        "Run 结束清掉 active 态后回落 wakeup 信号"
    );
}

#[test]
fn background_process_access_projects_summaries_logs_and_stop() {
    use tools::BackgroundProcessAccess as _;
    let registry =
        std::sync::Arc::new(crate::application::run::active_registry::ActiveRunRegistry::default());
    let runtime = BackgroundProcessRuntime::for_test(registry);

    let task_id = runtime.supervisor().register(
        background_process_identity(),
        "tool=Bash input=cargo test",
        SystemTime::now(),
    );
    runtime
        .supervisor()
        .mark_backgrounded(&task_id, None)
        .unwrap();

    // list：摘要词汇与工具名。
    let list = runtime.list_tasks();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].state, "backgrounded");
    assert_eq!(list[0].tool_name, "Bash");

    // status：详情可见。
    let detail = runtime
        .task_status(task_id.as_str())
        .expect("已登记任务详情可见");
    assert_eq!(detail.summary.task_id, task_id.as_str());

    // logs：尾部读取 + 增量（无文件任务回退终态文本——finish 未带输出
    // 则为空，语义上「无输出事实」不再经 ring buffer 采集）。
    let log = runtime.read_task_log(task_id.as_str(), None, 4096).unwrap();
    let delta = runtime
        .read_task_log(task_id.as_str(), Some(log.cursor), 4096)
        .unwrap();
    assert!(delta.text.is_empty(), "游标追平后增量为空");

    // stop：信号发出（token 取消），真实终态由执行体收口。
    let stop = runtime.stop_task(task_id.as_str()).expect("stop 成功");
    assert!(stop.signal_sent);
    assert_eq!(stop.state, "backgrounded");

    // 终态后 stop 幂等。
    runtime
        .supervisor()
        .finish(
            &task_id,
            crate::domain::background_process::BackgroundProcessTerminalKind::Success,
            Some("done".to_string()),
        )
        .unwrap();
    let stop_again = runtime.stop_task(task_id.as_str()).unwrap();
    assert!(!stop_again.signal_sent);
    assert_eq!(stop_again.state, "succeeded");
}

#[test]
fn background_process_access_rejects_invalid_task_ids() {
    use tools::BackgroundProcessAccess as _;
    let registry =
        std::sync::Arc::new(crate::application::run::active_registry::ActiveRunRegistry::default());
    let runtime = BackgroundProcessRuntime::for_test(registry);
    assert!(runtime.task_status("not-a-task-id").is_none());
    assert!(runtime.stop_task("garbage").is_err());
}

fn background_process_identity() -> context::ToolCallIdentityData {
    context::ToolCallIdentityData {
        session_id: context::SessionId::new("session-1"),
        run_id: sdk::RunId::new("run-1"),
        step_id: sdk::RunStepId::new("step-1"),
        runtime_call_id: "runtime-call-1".to_string(),
        provider_call_id: None,
        tool_name: "Bash".to_string(),
        call_index: 0,
        agent: false,
    }
}

// ── #252 PR3：账本持久化（终态快照落盘 / resume 恢复失效） ─────────────

#[tokio::test]
async fn ledger_persists_snapshots_and_restores_invalidation() {
    use storage::{ReadOutcomeData, StorageKeyData, StorageNamespaceData};

    let tempdir = tempfile::tempdir().unwrap();
    let blob = storage::wire_file_system_blob(tempdir.path()).unwrap();

    // 会话 A：登记两个任务——一个终态（Success）、一个 Backgrounded（进程将退出）。
    let registry_a =
        std::sync::Arc::new(crate::application::run::active_registry::ActiveRunRegistry::default());
    let runtime_a = BackgroundProcessRuntime::for_test(registry_a);
    let finished = runtime_a.supervisor().register(
        background_process_identity(),
        "tool=Bash input=done",
        SystemTime::now(),
    );
    runtime_a
        .supervisor()
        .finish(
            &finished,
            crate::domain::background_process::BackgroundProcessTerminalKind::Success,
            Some("ok".to_string()),
        )
        .unwrap();
    let orphaned = runtime_a.supervisor().register(
        background_process_identity(),
        "tool=Bash input=lost",
        SystemTime::now(),
    );
    runtime_a
        .supervisor()
        .mark_backgrounded(&orphaned, None)
        .unwrap();

    // 终态落盘：全量快照（ProcessCrashSafe）。
    runtime_a
        .bind_persistence(blob.clone(), "session-persist-1".to_string())
        .await
        .expect("绑定持久化");
    runtime_a.persist_snapshot().await.expect("快照落盘");

    // 会话 B（resume）：读快照恢复——终态保持、非终态标 Invalidated(ProcessExit)。
    let registry_b =
        std::sync::Arc::new(crate::application::run::active_registry::ActiveRunRegistry::default());
    let runtime_b = BackgroundProcessRuntime::for_test(registry_b);
    let restored = runtime_b
        .restore_from_snapshot(&blob, "session-persist-1")
        .await
        .expect("快照恢复");
    assert_eq!(restored, 2, "两条记录都应恢复");

    let snapshots = runtime_b.supervisor().snapshots();
    let restored_finished = snapshots
        .iter()
        .find(|record| record.task_id == finished)
        .expect("终态记录恢复");
    assert!(matches!(
        restored_finished.terminal_kind(),
        Some(crate::domain::background_process::BackgroundProcessTerminalKind::Success)
    ));
    let restored_orphan = snapshots
        .iter()
        .find(|record| record.task_id == orphaned)
        .expect("失联记录恢复");
    assert!(
        matches!(
            restored_orphan.terminal_kind(),
            Some(
                crate::domain::background_process::BackgroundProcessTerminalKind::Invalidated {
                    reason:
                        crate::domain::background_process::BackgroundInvalidationReason::ProcessExit
                }
            )
        ),
        "resume 时非终态后台进程必须失效"
    );

    // blob 侧可见：key 存在（Primary 读回非空）。
    let key = StorageKeyData::new(
        StorageNamespaceData::BackgroundProcess,
        vec!["session-persist-1".parse().unwrap()],
    )
    .unwrap();
    match blob
        .read(&key, storage::GenerationData::Primary)
        .await
        .unwrap()
    {
        ReadOutcomeData::Found(entry) => assert!(!entry.bytes().is_empty()),
        other => panic!("快照应可读回：{other:?}"),
    }
}

// ── 终态时长冻结（duration 不再随查询时刻增长） ───────────────────────

#[test]
fn task_summary_duration_freezes_after_terminal() {
    use tools::BackgroundProcessAccess as _;
    let registry =
        std::sync::Arc::new(crate::application::run::active_registry::ActiveRunRegistry::default());
    let runtime = BackgroundProcessRuntime::for_test(registry);

    let task_id = runtime.supervisor().register(
        background_process_identity(),
        "tool=Bash input=cargo test",
        SystemTime::now(),
    );
    runtime
        .supervisor()
        .mark_backgrounded(&task_id, None)
        .unwrap();
    runtime
        .supervisor()
        .finish(
            &task_id,
            crate::domain::background_process::BackgroundProcessTerminalKind::Success,
            Some("done".to_string()),
        )
        .unwrap();

    // 期望时长 = 终态快照固化时刻 - 创建时刻（精确对比，不依赖真实时间流逝）。
    let snapshot = runtime.supervisor().snapshot(&task_id).unwrap();
    let expected_ms = snapshot
        .finished_at
        .expect("终态记录已固化完成时刻")
        .duration_since(snapshot.started_at)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0);

    let list = runtime.list_tasks();
    assert_eq!(list.len(), 1);
    assert_eq!(
        list[0].duration_ms,
        Some(expected_ms),
        "终态时长应等于固化时刻差，而非查询时刻差"
    );

    // 二次查询（真实时间已推移）时长不变。
    let list_again = runtime.list_tasks();
    assert_eq!(
        list_again[0].duration_ms,
        Some(expected_ms),
        "终态时长不得随查询时刻增长"
    );
}

#[test]
fn task_summary_duration_tracks_now_for_running_task() {
    use tools::BackgroundProcessAccess as _;
    let registry =
        std::sync::Arc::new(crate::application::run::active_registry::ActiveRunRegistry::default());
    let runtime = BackgroundProcessRuntime::for_test(registry);
    runtime.supervisor().register(
        background_process_identity(),
        "tool=Bash input=running",
        SystemTime::now(),
    );

    let list = runtime.list_tasks();
    assert_eq!(list.len(), 1);
    assert!(
        list[0].duration_ms.is_some_and(|millis| millis < 60_000),
        "运行中任务时长按当前时刻计算"
    );
}

// ── spinner 活动数事件直达当前 chat 会话通道 ──────────────────────────

#[tokio::test]
async fn emit_active_count_delivers_event_to_bound_chat_sender() {
    let registry =
        std::sync::Arc::new(crate::application::run::active_registry::ActiveRunRegistry::default());
    let runtime = std::sync::Arc::new(BackgroundProcessRuntime::for_test(registry));

    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    let _binding = runtime.bind_chat_event_sender(sender);

    runtime.emit_active_count();
    let event = receiver.try_recv().expect("事件必须送达绑定的 chat 通道");
    assert!(
        matches!(
            &event,
            sdk::ChatEvent::BackgroundProcessCountChanged { active: 0 }
        ),
        "空账本活动数为 0，实际 {event:?}"
    );

    // 登记一个非终态任务后再发：活动数反映账本真相。
    runtime.supervisor().register(
        background_process_identity(),
        "tool=Bash input=live",
        SystemTime::now(),
    );
    runtime.emit_active_count();
    let event = receiver.try_recv().expect("第二次事件送达");
    assert!(
        matches!(
            &event,
            sdk::ChatEvent::BackgroundProcessCountChanged { active: 1 }
        ),
        "登记后活动数为 1，实际 {event:?}"
    );
}

#[test]
fn emit_active_count_without_bound_sender_is_noop() {
    let registry =
        std::sync::Arc::new(crate::application::run::active_registry::ActiveRunRegistry::default());
    let runtime = BackgroundProcessRuntime::for_test(registry);
    // 未绑定（如 Sub Run / 测试装配）：静默不发送，不 panic。
    runtime.emit_active_count();
}

// ── chat sender 绑定生命周期（RAII：chat 结束必须释放 sender） ─────────

#[tokio::test]
async fn chat_sender_binding_releases_on_drop_so_stream_closes() {
    let registry =
        std::sync::Arc::new(crate::application::run::active_registry::ActiveRunRegistry::default());
    let runtime = std::sync::Arc::new(BackgroundProcessRuntime::for_test(registry));

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let binding = runtime.bind_chat_event_sender(tx.clone());
    drop(tx);

    // 绑定存续期间改走 event 通道：事件可达。
    runtime.emit_active_count();
    assert!(rx.try_recv().is_ok(), "绑定期事件应可达");

    // chat 结束：guard drop 释放 sender → 全部 sender 已 drop，
    // receiver 关闭（真实挂起根因的最小复现：session 级 clone 遗忘
    // 使 `ChatStream::recv()` 永不返回 None）。
    drop(binding);
    assert!(
        rx.try_recv().is_err(),
        "绑定释放后不应再有可读事件（channel 已空且关闭）"
    );
    assert!(
        tokio::time::timeout(std::time::Duration::from_secs(5), rx.recv())
            .await
            .expect("sender 释放后 stream 必须关闭（不得挂起）")
            .is_none(),
        "全部 sender 释放后 stream 返回 None"
    );
}

#[tokio::test]
async fn rebinding_replaces_previous_sender_and_stale_guard_keeps_new_binding() {
    let registry =
        std::sync::Arc::new(crate::application::run::active_registry::ActiveRunRegistry::default());
    let runtime = std::sync::Arc::new(BackgroundProcessRuntime::for_test(registry));

    let (first_tx, _first_rx) = tokio::sync::mpsc::unbounded_channel();
    let first_binding = runtime.bind_chat_event_sender(first_tx);
    // 新 chat 覆盖绑定。
    let (second_tx, mut second_rx) = tokio::sync::mpsc::unbounded_channel();
    let _second_binding = runtime.bind_chat_event_sender(second_tx);

    // 旧 guard 释放不得清掉新绑定（generation 校验）。
    drop(first_binding);
    runtime.emit_active_count();
    assert!(
        second_rx.try_recv().is_ok(),
        "旧 guard 释放后新绑定仍须可达"
    );
}

// ── 时长自工具派发起算（含前台等待段） ───────────────────────────────

#[test]
fn task_summary_duration_counts_from_dispatch_time() {
    use tools::BackgroundProcessAccess as _;
    let registry =
        std::sync::Arc::new(crate::application::run::active_registry::ActiveRunRegistry::default());
    let runtime = BackgroundProcessRuntime::for_test(registry);

    // 派发时刻在 12 秒前（前台等待 + 后台存活总计 12s）。
    let dispatch_time = SystemTime::now() - std::time::Duration::from_secs(12);
    runtime.supervisor().register(
        background_process_identity(),
        "tool=Bash input=long",
        dispatch_time,
    );

    let list = runtime.list_tasks();
    assert_eq!(list.len(), 1);
    let duration_ms = list[0].duration_ms.expect("运行中任务有时长");
    assert!(
        duration_ms >= 12_000,
        "时长自派发时刻起算（覆盖前台等待段）：{duration_ms}ms"
    );
}

// ── Run 收口滞留事实检测（#252 注入确认制兜底） ──────────────────────

#[tokio::test]
async fn stranded_facts_detection_signals_wakeup_only_when_unconfirmed() {
    let registry =
        std::sync::Arc::new(crate::application::run::active_registry::ActiveRunRegistry::default());
    let runtime = std::sync::Arc::new(BackgroundProcessRuntime::for_test(registry));
    let mut waiter = runtime.take_wakeup_waiter().expect("session 首次取等待端");

    // 无事实：不发信号。
    runtime.signal_wakeup_for_stranded_facts();
    assert!(waiter.try_wait().is_none(), "无滞留事实不得发信号");

    // 未确认完成事实（收口临界窗口被 peek 未注入）：补发 wakeup。
    let task_id = runtime.supervisor().register(
        background_process_identity(),
        "tool=Bash input=stranded",
        SystemTime::now(),
    );
    runtime
        .supervisor()
        .finish(
            &task_id,
            crate::domain::background_process::BackgroundProcessTerminalKind::Success,
            Some("done".to_string()),
        )
        .unwrap();
    runtime.signal_wakeup_for_stranded_facts();
    assert!(
        waiter.try_wait().is_some(),
        "滞留事实必须立即补发 wakeup（不等下一个事件捎带）"
    );

    // 同批事实（无新增、未确认）：one-shot——消费一次即达，不再重弹。
    runtime.signal_wakeup_for_stranded_facts();
    assert!(
        waiter.try_wait().is_none(),
        "同批滞留事实只弹一次（消费一次即达）——wakeup Run 失败收口后不得无限唤醒风暴"
    );

    // 新事实到达：新批次，弹一次。
    let another_id = runtime.supervisor().register(
        background_process_identity(),
        "tool=Bash input=stranded-2",
        SystemTime::now(),
    );
    runtime
        .supervisor()
        .finish(
            &another_id,
            crate::domain::background_process::BackgroundProcessTerminalKind::Success,
            Some("done-2".to_string()),
        )
        .unwrap();
    runtime.signal_wakeup_for_stranded_facts();
    assert!(
        waiter.try_wait().is_some(),
        "新事实形成新批次，允许再弹一次"
    );

    // 新批次弹过后又稳定：不再弹。
    runtime.signal_wakeup_for_stranded_facts();
    assert!(waiter.try_wait().is_none(), "新批次同样 one-shot");
}

#[test]
fn background_process_status_reports_log_file_bytes() {
    use tools::BackgroundProcessAccess as _;
    let registry =
        std::sync::Arc::new(crate::application::run::active_registry::ActiveRunRegistry::default());
    let runtime = BackgroundProcessRuntime::for_test(registry);
    let base = std::env::temp_dir().join(format!(
        "bgp-status-bytes-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let process_id = share::ids::BackgroundProcessId::new_v7();
    let (log, mut stdout, _stderr) =
        crate::application::background_process::log_file::TaskLogFile::open(
            &base,
            "sess-status-bytes",
            &process_id,
        )
        .unwrap();
    use std::io::Write as _;
    stdout.write_all(&vec![b'z'; 366]).unwrap();
    drop(stdout);
    let task_id = runtime.supervisor().register_direct(
        process_id,
        log.path().to_path_buf(),
        background_process_identity(),
        "command=status-bytes",
        tokio_util::sync::CancellationToken::new(),
        SystemTime::now(),
    );

    let detail = runtime.task_status(task_id.as_str()).expect("详情可见");
    assert_eq!(
        detail.total_written_bytes, 366,
        "total_written_bytes = 任务日志文件实际大小（缺陷③：不再硬编码 0）"
    );

    let _ = std::fs::remove_dir_all(&base);
}
