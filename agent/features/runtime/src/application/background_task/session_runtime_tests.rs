use super::*;

#[test]
fn notify_route_targets_active_main_run_when_present() {
    let registry =
        std::sync::Arc::new(crate::application::run::active_registry::ActiveRunRegistry::default());
    let runtime = BackgroundTaskRuntime::for_test(registry.clone());

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
    let runtime = BackgroundTaskRuntime::for_test(registry.clone());
    let run_id = sdk::RunId::new_v7();
    registry.activate_main_for_test(run_id.clone());

    registry.clear_for_test(&run_id);
    assert!(
        matches!(runtime.notify_route(), BackgroundNotifyRoute::WakeupSignal),
        "Run 结束清掉 active 态后回落 wakeup 信号"
    );
}

#[test]
fn background_task_access_projects_summaries_logs_and_stop() {
    use tools::BackgroundTaskAccess as _;
    let registry =
        std::sync::Arc::new(crate::application::run::active_registry::ActiveRunRegistry::default());
    let runtime = BackgroundTaskRuntime::for_test(registry);

    let task_id = runtime
        .supervisor()
        .register(background_task_identity(), "tool=Bash input=cargo test");
    runtime.supervisor().record_output(&task_id, b"building\n");
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

    // logs：尾部读取 + 增量。
    let log = runtime.read_task_log(task_id.as_str(), None, 4096).unwrap();
    assert!(log.text.contains("building"));
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
            crate::domain::background_task::BackgroundTaskTerminalKind::Success,
            Some("done".to_string()),
        )
        .unwrap();
    let stop_again = runtime.stop_task(task_id.as_str()).unwrap();
    assert!(!stop_again.signal_sent);
    assert_eq!(stop_again.state, "succeeded");
}

#[test]
fn background_task_access_rejects_invalid_task_ids() {
    use tools::BackgroundTaskAccess as _;
    let registry =
        std::sync::Arc::new(crate::application::run::active_registry::ActiveRunRegistry::default());
    let runtime = BackgroundTaskRuntime::for_test(registry);
    assert!(runtime.task_status("not-a-task-id").is_none());
    assert!(runtime.stop_task("garbage").is_err());
}

fn background_task_identity() -> context::ToolCallIdentityData {
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
    use storage::{AtomicBlobPort, ReadOutcomeData, StorageKeyData, StorageNamespaceData};

    let tempdir = tempfile::tempdir().unwrap();
    let blob = storage::wire_file_system_blob(tempdir.path()).unwrap();

    // 会话 A：登记两个任务——一个终态（Success）、一个 Backgrounded（进程将退出）。
    let registry_a =
        std::sync::Arc::new(crate::application::run::active_registry::ActiveRunRegistry::default());
    let runtime_a = BackgroundTaskRuntime::for_test(registry_a);
    let finished = runtime_a
        .supervisor()
        .register(background_task_identity(), "tool=Bash input=done");
    runtime_a
        .supervisor()
        .finish(
            &finished,
            crate::domain::background_task::BackgroundTaskTerminalKind::Success,
            Some("ok".to_string()),
        )
        .unwrap();
    let orphaned = runtime_a
        .supervisor()
        .register(background_task_identity(), "tool=Bash input=lost");
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
    let runtime_b = BackgroundTaskRuntime::for_test(registry_b);
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
        Some(crate::domain::background_task::BackgroundTaskTerminalKind::Success)
    ));
    let restored_orphan = snapshots
        .iter()
        .find(|record| record.task_id == orphaned)
        .expect("失联记录恢复");
    assert!(
        matches!(
            restored_orphan.terminal_kind(),
            Some(
                crate::domain::background_task::BackgroundTaskTerminalKind::Invalidated {
                    reason:
                        crate::domain::background_task::BackgroundInvalidationReason::ProcessExit
                }
            )
        ),
        "resume 时非终态后台任务必须失效"
    );

    // blob 侧可见：key 存在（Primary 读回非空）。
    let key = StorageKeyData::new(
        StorageNamespaceData::BackgroundTask,
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
    use tools::BackgroundTaskAccess as _;
    let registry =
        std::sync::Arc::new(crate::application::run::active_registry::ActiveRunRegistry::default());
    let runtime = BackgroundTaskRuntime::for_test(registry);

    let task_id = runtime
        .supervisor()
        .register(background_task_identity(), "tool=Bash input=cargo test");
    runtime
        .supervisor()
        .mark_backgrounded(&task_id, None)
        .unwrap();
    runtime
        .supervisor()
        .finish(
            &task_id,
            crate::domain::background_task::BackgroundTaskTerminalKind::Success,
            Some("done".to_string()),
        )
        .unwrap();

    // 期望时长 = 终态快照固化时刻 - 创建时刻（精确对比，不依赖真实时间流逝）。
    let snapshot = runtime.supervisor().snapshot(&task_id).unwrap();
    let expected_ms = snapshot
        .finished_at
        .expect("终态记录已固化完成时刻")
        .duration_since(snapshot.created_at)
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
    use tools::BackgroundTaskAccess as _;
    let registry =
        std::sync::Arc::new(crate::application::run::active_registry::ActiveRunRegistry::default());
    let runtime = BackgroundTaskRuntime::for_test(registry);
    runtime
        .supervisor()
        .register(background_task_identity(), "tool=Bash input=running");

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
    let runtime = BackgroundTaskRuntime::for_test(registry);

    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    runtime.bind_chat_event_sender(sender);

    runtime.emit_active_count();
    let event = receiver.try_recv().expect("事件必须送达绑定的 chat 通道");
    assert!(
        matches!(
            &event,
            sdk::ChatEvent::BackgroundTaskCountChanged { active: 0 }
        ),
        "空账本活动数为 0，实际 {event:?}"
    );

    // 登记一个非终态任务后再发：活动数反映账本真相。
    runtime
        .supervisor()
        .register(background_task_identity(), "tool=Bash input=live");
    runtime.emit_active_count();
    let event = receiver.try_recv().expect("第二次事件送达");
    assert!(
        matches!(
            &event,
            sdk::ChatEvent::BackgroundTaskCountChanged { active: 1 }
        ),
        "登记后活动数为 1，实际 {event:?}"
    );
}

#[test]
fn emit_active_count_without_bound_sender_is_noop() {
    let registry =
        std::sync::Arc::new(crate::application::run::active_registry::ActiveRunRegistry::default());
    let runtime = BackgroundTaskRuntime::for_test(registry);
    // 未绑定（如 Sub Run / 测试装配）：静默不发送，不 panic。
    runtime.emit_active_count();
}
