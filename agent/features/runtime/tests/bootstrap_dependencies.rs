use std::sync::Arc;

use context::{compose_session_task_capture, LegacyTaskCapture};
use runtime::RuntimeBootstrapDependencies;

#[derive(Clone)]
struct TestOpener;

#[async_trait::async_trait]
impl memory::api::MemoryOpener for TestOpener {
    async fn open_memory(
        &self,
        _key: &memory::api::ProjectMemoryKey,
        _config: &share::config::MemoryConfig,
    ) -> Result<Arc<dyn memory::api::MemoryPort>, memory::api::MemoryOpenerError> {
        Ok(Arc::new(memory::api::NoOpMemory))
    }
    fn boxed_clone(&self) -> Box<dyn memory::api::MemoryOpener> {
        Box::new(self.clone())
    }
}

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

    let wiring = context::wire_main_session(context::MainSessionDependencies {
        workspace: workspace.clone(),
        task_persist: task.persist(),
        config_reader: config.reader(),
        config_participant: config.participant(),
        memory_opener: Box::new(TestOpener),
    })
    .await
    .unwrap();
    let dependencies = RuntimeBootstrapDependencies::new(
        workspace,
        wiring,
        provider::wire_provider(),
        tools::wire_tools(),
        Arc::new(policy::AllowAllPolicy),
        access.clone(),
        capture.clone(),
    );

    assert!(Arc::ptr_eq(&dependencies.task_access(), &access));
    assert!(Arc::ptr_eq(&dependencies.session_tasks(), &capture));
}
