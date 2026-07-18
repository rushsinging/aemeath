use std::sync::Arc;

use context::{compose_session_task_capture, LegacyTaskCapture};
use runtime::{RuntimeBootstrapDependencies, RuntimeConfigDependencies};

struct NoopReflectionHistory;

#[async_trait::async_trait]
impl memory::api::ReflectionHistoryQuery for NoopReflectionHistory {
    async fn list(
        &self,
        _limit: usize,
    ) -> Result<Vec<memory::api::ReflectionRecord>, memory::api::MemoryError> {
        Ok(Vec::new())
    }
}

#[async_trait::async_trait]
impl memory::api::ReflectionHistoryStore for NoopReflectionHistory {
    async fn append(
        &self,
        _record: &memory::api::ReflectionRecord,
    ) -> Result<(), memory::api::MemoryError> {
        Ok(())
    }

    async fn upsert(
        &self,
        _record: &memory::api::ReflectionRecord,
    ) -> Result<(), memory::api::MemoryError> {
        Ok(())
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

    let history: Arc<dyn memory::ReflectionHistoryStore> = Arc::new(NoopReflectionHistory);

    let dependencies = RuntimeBootstrapDependencies::new(
        workspace,
        RuntimeConfigDependencies::new(config.reader(), config.query(), config.writer()),
        Arc::new(memory::NoOpMemory),
        history.clone(),
        provider::wire_provider(),
        tools::wire_tools(),
        Arc::new(policy::AllowAllPolicy),
        access.clone(),
        capture.clone(),
    );

    assert!(Arc::ptr_eq(&dependencies.reflection_history(), &history));
    assert!(Arc::ptr_eq(&dependencies.task_access(), &access));
    assert!(Arc::ptr_eq(&dependencies.session_tasks(), &capture));
}
