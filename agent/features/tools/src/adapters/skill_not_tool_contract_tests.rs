//! Contract test pinning the Issue #912 invariant: **Skill is NOT a Tool**.
//!
//! A Skill is a reusable prompt template surfaced via a dedicated system slot,
//! never an entry in the Tool catalog and never an executable `ToolInvocation`.
//! This test verifies that the production builtin catalog/execution wiring
//! (`wire_builtin_catalog_execution`) does not expose or execute Skill for the
//! `main` and `sub-agent` scopes.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde_json::json;

use crate::composition::wire_builtin_catalog_execution;
use crate::domain::memory_source::MemoryPortSource;
use crate::domain::published_language::ToolOutcome as ExecutionOutcome;
use crate::domain::{
    CancellationSignal, ExecutionScope, FixedGuidance, FixedPlanMode, MutexReadSet,
    RegistryScopeName, ToolErrorKind, ToolExecutionContext, ToolExecutionPorts, ToolInvocation,
    ToolName, ToolProfileName, WorkspaceReadAccess,
};

// ── test doubles ────────────────────────────────────────────────────────

struct NeverCancelled;

#[async_trait]
impl CancellationSignal for NeverCancelled {
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

/// Cheap, sync source returning a fresh in-memory Memory port.
fn test_memory_source() -> Arc<dyn MemoryPortSource> {
    struct TestSource;
    impl MemoryPortSource for TestSource {
        fn current(&self) -> Arc<dyn memory::MemoryPort> {
            Arc::new(
                memory::InMemoryMemory::new(memory::MemoryPolicy::default())
                    .expect("valid default policy"),
            )
        }
    }
    Arc::new(TestSource)
}

/// Assemble the production builtin catalog/execution wiring against an empty
/// workspace. Skill is no longer passed as a tool-registration parameter (#912).
fn assembled() -> (
    crate::composition::CatalogExecutionWiring,
    Arc<dyn project::WorkspaceRead>,
) {
    let workspace = tempfile::tempdir().expect("workspace");
    let wiring_views = project::wire_production_workspace(workspace.path().to_path_buf())
        .expect("workspace wiring");
    let views = wiring_views.into_views();

    let task_access: Arc<dyn task::TaskAccess> = Arc::new(task::TaskStore::new());
    let workspace_control = views.control();
    let workspace_read = views.read();

    let wiring =
        wire_builtin_catalog_execution(task_access, test_memory_source(), workspace_control)
            .expect("builtin catalog/execution wiring");

    // Leak the tempdir so the workspace root survives the test body; tests are
    // short-lived processes and the OS reclaims the directory on exit.
    std::mem::forget(workspace);

    (wiring, workspace_read)
}

/// Build a bound execution context + matching invocation for the given
/// registry scope / profile pair. The run id is unique per call so the same
/// wiring can bind more than one context.
fn bound_invocation(
    wiring: &crate::composition::CatalogExecutionWiring,
    read: &Arc<dyn project::WorkspaceRead>,
    run_id: &str,
    scope: &str,
    profile: &str,
) -> ToolInvocation {
    let execution_scope =
        ExecutionScope::builder(run_id, read.workspace_id(), read.current_workspace_root())
            .registry_scope(RegistryScopeName::new(scope))
            .profile(ToolProfileName::new(profile))
            .build();

    let context = ToolExecutionContext::new(
        execution_scope.clone(),
        ToolExecutionPorts::new(
            Arc::new(NeverCancelled),
            WorkspaceReadAccess::new(read.clone()),
            Arc::new(MutexReadSet(Arc::new(Mutex::new(HashSet::new())))),
            Arc::new(FixedPlanMode(None)),
            Arc::new(memory::NoOpMemory),
            Arc::new(FixedGuidance {
                language: "en".into(),
            }),
        ),
    );
    wiring.bind(context).expect("bind context");

    ToolInvocation::new("Skill", json!({"skill": "commit"}), execution_scope)
}

fn assert_unavailable(outcome: ExecutionOutcome) {
    assert!(
        matches!(outcome, ExecutionOutcome::Failure(ref f) if f.kind == ToolErrorKind::ToolUnavailable),
        "Skill must be ToolUnavailable, got: {outcome:?}"
    );
}

#[tokio::test]
async fn skill_is_not_a_tool_in_catalog_or_execution_for_main() {
    let (wiring, read) = assembled();

    // main-full / main: Skill must NOT be a catalog tool.
    let snapshot = wiring
        .catalog()
        .snapshot(
            &RegistryScopeName::new("main"),
            &ToolProfileName::new("main-full"),
        )
        .expect("main snapshot");
    assert!(
        snapshot.find(&ToolName::new("Skill")).is_none(),
        "main-full catalog must not expose Skill as a tool"
    );

    // Bound execution context: invoking Skill must be ToolUnavailable.
    let invocation = bound_invocation(
        &wiring,
        &read,
        "skill-is-not-tool-main",
        "main",
        "main-full",
    );
    let outcome = wiring
        .execution()
        .execute(invocation, &NeverCancelled)
        .await;
    assert_unavailable(outcome);
}

#[tokio::test]
async fn skill_is_not_a_tool_in_catalog_or_execution_for_sub_agent() {
    let (wiring, read) = assembled();

    // sub-agent-restricted / sub-agent: Skill must NOT be a catalog tool.
    let snapshot = wiring
        .catalog()
        .snapshot(
            &RegistryScopeName::new("sub-agent"),
            &ToolProfileName::new("sub-agent-restricted"),
        )
        .expect("sub-agent snapshot");
    assert!(
        snapshot.find(&ToolName::new("Skill")).is_none(),
        "sub-agent-restricted catalog must not expose Skill as a tool"
    );

    // Bound execution context: invoking Skill must be ToolUnavailable.
    let invocation = bound_invocation(
        &wiring,
        &read,
        "skill-is-not-tool-sub",
        "sub-agent",
        "sub-agent-restricted",
    );
    let outcome = wiring
        .execution()
        .execute(invocation, &NeverCancelled)
        .await;
    assert_unavailable(outcome);
}
