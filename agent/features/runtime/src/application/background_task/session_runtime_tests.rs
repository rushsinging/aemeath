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
