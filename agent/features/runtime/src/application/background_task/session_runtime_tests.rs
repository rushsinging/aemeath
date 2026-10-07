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
