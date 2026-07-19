use crate::domain::{
    CancellationSignal, ExecutionScope, FixedGuidance, FixedPlanMode, MutexReadSet,
    ToolExecutionContext, ToolExecutionPorts, WorkspaceReadAccess,
};
use async_trait::async_trait;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

struct TestCancellation;

#[async_trait]
impl CancellationSignal for TestCancellation {
    fn is_cancelled(&self) -> bool {
        false
    }

    async fn cancelled(&self) {
        std::future::pending::<()>().await;
    }

    fn child_signal(&self) -> Arc<dyn CancellationSignal> {
        Arc::new(Self)
    }
}

struct WorkspaceTestPorts {
    control: Arc<dyn project::WorkspaceControl>,
    persist: Arc<dyn project::WorkspacePersist>,
}

fn workspace_ports() -> &'static Mutex<HashMap<String, WorkspaceTestPorts>> {
    static PORTS: OnceLock<Mutex<HashMap<String, WorkspaceTestPorts>>> = OnceLock::new();
    PORTS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Adapter integration fixture backed by the real Project production wiring.
pub(crate) fn production_execution_context(root: PathBuf) -> ToolExecutionContext {
    let views = project::wire_production_workspace(root)
        .expect("workspace initialization")
        .into_views();
    let read = views.read();
    let workspace_id = read.workspace_id();
    let workspace_root = read.current_workspace_root();
    workspace_ports()
        .lock()
        .expect("workspace test ports")
        .insert(
            workspace_id.as_str().to_string(),
            WorkspaceTestPorts {
                control: views.control(),
                persist: views.persist(),
            },
        );

    ToolExecutionContext::new(
        ExecutionScope::builder("test-run", workspace_id, workspace_root).build(),
        ToolExecutionPorts::new(
            Arc::new(TestCancellation),
            WorkspaceReadAccess::new(read),
            Arc::new(MutexReadSet(Arc::new(Mutex::new(HashSet::new())))),
            Arc::new(FixedPlanMode(None)),
            Arc::new(memory::NoOpMemory),
            Arc::new(FixedGuidance {
                language: "en".into(),
            }),
        ),
    )
}

pub(crate) fn production_workspace_control(
    context: &ToolExecutionContext,
) -> Arc<dyn project::WorkspaceControl> {
    workspace_ports()
        .lock()
        .expect("workspace test ports")
        .get(context.scope().workspace_id().as_str())
        .expect("workspace control")
        .control
        .clone()
}

pub(crate) fn production_workspace_persist(
    context: &ToolExecutionContext,
) -> Arc<dyn project::WorkspacePersist> {
    workspace_ports()
        .lock()
        .expect("workspace test ports")
        .get(context.scope().workspace_id().as_str())
        .expect("workspace persist")
        .persist
        .clone()
}
