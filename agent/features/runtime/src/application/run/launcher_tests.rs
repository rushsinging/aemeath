#[test]
fn launcher_passes_per_run_activity_coordinator_to_engine() {
    let source = include_str!("launcher.rs");

    assert!(source.contains("let activities = context.activities().clone();"));
    assert!(source.contains("execute_prepared_loop(run, execution, context, activities,"));
}

#[test]
fn legacy_launcher_entries_are_retired_after_instance_migration() {
    let source = include_str!("launcher.rs");
    assert!(!source.contains("pub async fn launch<P>"));
    assert!(!source.contains("pub async fn launch_prepared("));
    assert!(!source.contains("迁移期兼容入口"));
}

#[test]
fn launcher_consumes_run_instance_without_requiring_callers_to_unpack_runtime_state() {
    let launcher = include_str!("launcher.rs");
    let main = include_str!("../loop_engine/chat/session_driver/run_launch.rs");
    let derived_setup = include_str!("derived/setup.rs");
    let derived_loop = include_str!("derived/loop_run.rs");

    assert!(launcher.contains("instance: &mut RunInstance"));
    assert!(!launcher.contains("mut run: Run"));
    assert!(!launcher.contains("execution: &mut RunExecutionState"));
    assert!(!main.contains("run_instance.into_parts()"));
    assert!(!derived_setup.contains("run_instance.into_parts()"));
    assert!(derived_loop.contains("instance: &mut RunInstance"));
}

#[test]
fn launcher_registers_only_root_run_as_current_foreground_run() {
    let launcher = include_str!("launcher.rs");

    assert!(launcher.contains("instance.run().parent_id().is_none()"));
    assert!(launcher.contains("active_run.activate_main(run_id.clone(), cancel.clone())"));
    assert!(launcher.contains("active_run.activate_child(run_id.clone(), cancel.clone())"));
}
