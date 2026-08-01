use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use crate::application::run::workspace::RuntimeWorkspaceAccess;

fn test_workspaces() -> &'static Mutex<HashMap<String, RuntimeWorkspaceAccess>> {
    static WORKSPACES: OnceLock<Mutex<HashMap<String, RuntimeWorkspaceAccess>>> = OnceLock::new();
    WORKSPACES.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(crate) fn test_runtime_workspace_access() -> RuntimeWorkspaceAccess {
    let root = std::env::temp_dir().join(format!(
        "aemeath-runtime-workspace-{}",
        uuid::Uuid::now_v7()
    ));
    std::fs::create_dir_all(&root).expect("create workspace root");
    let views = project::wire_production_workspace(root)
        .expect("workspace initialization")
        .into_views();
    RuntimeWorkspaceAccess::new(views)
}

pub(crate) fn test_tool_execution_context(
    root: std::path::PathBuf,
    cancel: tokio_util::sync::CancellationToken,
) -> tools::ToolExecutionContext {
    let views = project::wire_production_workspace(root.clone())
        .expect("workspace initialization")
        .into_views();
    let workspace = RuntimeWorkspaceAccess::new(views.clone());
    test_workspaces().lock().expect("test workspaces").insert(
        views.read().workspace_id().as_str().to_string(),
        workspace.clone(),
    );
    tools::ToolExecutionContext::new(
        tools::ExecutionScope::builder("test-run", views.read().workspace_id(), root).build(),
        tools::ToolExecutionPorts::new(
            Arc::new(crate::application::run::context::RunCancellationScope::from_token(cancel)),
            workspace.read_access(),
            Arc::new(tools::MutexReadSet(Arc::new(std::sync::Mutex::new(
                std::collections::HashSet::new(),
            )))),
            Arc::new(tools::FixedPlanMode(None)),
            Arc::new(memory::NoOpMemory),
            Arc::new(tools::FixedGuidance {
                language: "en".into(),
            }),
        ),
    )
}

pub(crate) fn runtime_workspace(ctx: &tools::ToolExecutionContext) -> RuntimeWorkspaceAccess {
    test_workspaces()
        .lock()
        .expect("test workspaces")
        .get(ctx.scope().workspace_id().as_str())
        .expect("context workspace backing")
        .clone()
}

pub(crate) fn workspace_persist(
    ctx: &tools::ToolExecutionContext,
) -> Arc<dyn project::WorkspacePersist> {
    runtime_workspace(ctx).persist()
}
