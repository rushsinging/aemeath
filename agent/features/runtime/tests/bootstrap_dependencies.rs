use std::sync::Arc;

use context::{compose_session_task_capture, LegacyTaskCapture};
use runtime::RuntimeBootstrapDependencies;

#[tokio::test]
async fn bootstrap_dependencies_preserve_injected_task_views() {
    let temp = tempfile::tempdir().unwrap();
    let config = config::wire_project_config(temp.path()).await.unwrap();
    let workspace = project::wire_production_workspace(temp.path().to_path_buf())
        .unwrap()
        .into_views();
    let task = task::wire_task();
    let access = task.access();
    let capture: Arc<dyn LegacyTaskCapture> = compose_session_task_capture(task.persist());

    let wiring = context::test_support::wire_in_memory(
        &workspace,
        task.persist(),
        config.reader(),
        config.participant(),
    )
    .await;

    let dependencies = RuntimeBootstrapDependencies::new(
        workspace,
        wiring,
        provider::wire_provider(),
        tools::wire_tools(),
        access.clone(),
        capture.clone(),
    );

    assert!(Arc::ptr_eq(&dependencies.task_access(), &access));
    assert!(Arc::ptr_eq(&dependencies.session_tasks(), &capture));
}
