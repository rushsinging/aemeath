//! Composition→Runtime contract tests (#1385 Task 8).
//!
//! Tests in this file live inside the `composition::runtime` module, giving
//! direct access to `wire_runtime_tool_assembly` and `RuntimeToolAssembly`.
//! They prove that production composition produces working
//! catalog/execution/binding ports. The `tool_result_materializer` and
//! `active_run` fields on `RuntimeToolAssembly` are constructable but have
//! no public functional API — their wiring is verified by struct field
//! existence only.

use std::sync::Arc;

// wire_runtime_tool_assembly, RuntimeToolAssembly, WiringMemoryPortSource, etc.
// are accessible via `super::` because this module is declared inside runtime.rs
// with `#[cfg(test)]` (which grants access to private parent items).

// ─── Test doubles: minimal, no unnecessary abstraction ────────────────

struct TestMemoryPortSource {
    memory: Arc<dyn memory::MemoryPort>,
}

impl tools::MemoryPortSource for TestMemoryPortSource {
    fn current(&self) -> Arc<dyn memory::MemoryPort> {
        self.memory.clone()
    }
}

fn noop_memory_source() -> Arc<dyn tools::MemoryPortSource> {
    Arc::new(TestMemoryPortSource {
        memory: Arc::new(memory::api::NoOpMemory),
    })
}

struct NeverCancelled;

#[async_trait::async_trait]
impl tools::CancellationSignal for NeverCancelled {
    fn is_cancelled(&self) -> bool {
        false
    }
    async fn cancelled(&self) {
        std::future::pending::<()>().await
    }
    fn child_signal(&self) -> Arc<dyn tools::CancellationSignal> {
        Arc::new(Self)
    }
}

fn cancellation() -> Arc<dyn tools::CancellationSignal> {
    Arc::new(NeverCancelled)
}

// ─── Helpers ──────────────────────────────────────────────────────────

fn temp_workspace() -> (tempfile::TempDir, project::WorkspaceWiring) {
    let temp = tempfile::tempdir().expect("create temp dir");
    let root = temp.path().join("root");
    std::fs::create_dir_all(&root).expect("create root dir");
    let wiring = project::wire_production_workspace(root).expect("wire workspace");
    (temp, wiring)
}

/// Returns the canonical workspace root path.
fn workspace_root(workspace: &project::WorkspaceWiring) -> std::path::PathBuf {
    workspace
        .read()
        .project_identity()
        .initial_cwd
        .clone()
        .into()
}

/// Build a `ToolExecutionContext` for the given workspace and run_id.
/// The scope's `workspace_root` is set to the project's initial_cwd so
/// invocation scopes built with the same root pass the equality check
/// in `BoundExecutionContexts::resolve`.
fn tool_context(workspace: &project::WorkspaceWiring, run_id: &str) -> tools::ToolExecutionContext {
    let root: std::path::PathBuf = workspace_root(workspace);
    let scope =
        tools::ExecutionScope::builder(run_id, workspace.read().workspace_id(), root).build();
    let ports = tools::ToolExecutionPorts::new(
        cancellation(),
        tools::WorkspaceReadAccess::new(workspace.read()),
        Arc::new(tools::MutexReadSet(Arc::new(std::sync::Mutex::new(
            std::collections::HashSet::new(),
        )))),
        Arc::new(tools::FixedPlanMode(None)),
        Arc::new(memory::api::NoOpMemory),
        Arc::new(tools::FixedGuidance {
            language: "en".to_string(),
        }),
    );
    tools::ToolExecutionContext::new(scope, ports)
}

/// Create a temp file inside the workspace root, write content, return its path.
fn write_file_in_root(
    workspace: &project::WorkspaceWiring,
    name: &str,
    content: &str,
) -> std::path::PathBuf {
    let root: std::path::PathBuf = workspace_root(workspace);
    let file_path = root.join(name);
    std::fs::write(&file_path, content).expect("write test file in workspace root");
    file_path
}

// ═══════════════════════════════════════════════════════════════════════
// Test 1: wire_runtime_tool_assembly integrates catalog/execution/binding
// ═══════════════════════════════════════════════════════════════════════

/// Calls the private `wire_runtime_tool_assembly` — the exact function used
/// by production `from_args_with_gateways` — and verifies:
///
///   - Catalog, execution, and binding ports are populated and functional
///     (snapshot returns real built-in tools; bind→execute→unbind works).
///   - `tool_result_materializer` and `active_run` fields are constructable
///     (no public functional API — verified by struct field existence).
///
/// This is the strongest composition→runtime contract test: it exercises
/// the exact same production function that `from_args_with_gateways` calls.
///
/// Uses a temp directory for `agents_dir` injected directly into
/// `wire_runtime_tool_assembly` — no env var manipulation, no global races.
#[tokio::test]
async fn wire_runtime_tool_assembly_produces_working_catalog_execution_binding() {
    let env_temp = tempfile::tempdir().expect("temp dir for agents dir");

    let (_temp, workspace) = temp_workspace();
    let task_wiring = task::wire_task();
    let memory_source = noop_memory_source();

    let snapshot = share::config::domain::snapshot::ConfigSnapshot::new(
        share::config::domain::config::Config::default(),
    );

    let assembly = super::wire_runtime_tool_assembly(
        task_wiring.access(),
        memory_source,
        workspace.control(),
        &snapshot,
        env_temp.path(),
    )
    .expect("wire_runtime_tool_assembly must succeed");

    // ── All three tool ports are populated (fields exist, not null) ──
    let catalog = &assembly.catalog;
    let execution = &assembly.execution;
    let binding = &assembly.binding;

    // ── Catalog is functional ──
    let snap = catalog
        .snapshot(
            &tools::RegistryScopeName::new("main"),
            &tools::ToolProfileName::new("main-full"),
        )
        .expect("catalog snapshot");
    let tool_names: Vec<&str> = snap.tools.iter().map(|t| t.name.as_str()).collect();
    assert!(tool_names.contains(&"Bash"));
    assert!(tool_names.contains(&"Read"));

    // ── Binding + execution works ──
    let ctx = tool_context(&workspace, "run-assembly");
    binding.bind(ctx).expect("bind context via assembly");

    let file_path = write_file_in_root(&workspace, "assembly.txt", "assembly test content");
    let root: std::path::PathBuf = workspace_root(&workspace);

    let scope =
        tools::ExecutionScope::builder("run-assembly", workspace.read().workspace_id(), root)
            .build();
    let invocation = tools::ToolInvocation::new(
        "Read",
        serde_json::json!({"file_path": file_path.to_string_lossy()}),
        scope,
    );
    let outcome = execution.execute(invocation, &*cancellation()).await;
    assert!(
        outcome.is_success(),
        "execution via assembly must succeed, got: {:?}",
        outcome
    );

    // ── Unbind → execution must fail (ports share backing) ──
    binding.unbind("run-assembly");
    let root2: std::path::PathBuf = workspace_root(&workspace);
    let scope2 =
        tools::ExecutionScope::builder("run-assembly", workspace.read().workspace_id(), root2)
            .build();
    let invocation2 = tools::ToolInvocation::new(
        "Read",
        serde_json::json!({"file_path": file_path.to_string_lossy()}),
        scope2,
    );
    let outcome2 = execution.execute(invocation2, &*cancellation()).await;
    assert!(
        outcome2.is_failure(),
        "execution after unbind must fail, got: {:?}",
        outcome2
    );

    // ── tool_result_materializer and active_run are constructable ──
    // ToolResultMaterializer and ActiveRunRegistry have no public functional
    // query API. Their wiring is verified by struct field existence:
    // the fields are `Arc<T>` and the assembly constructs them without error.
    let _ = &assembly.tool_result_materializer;
    let _ = &assembly.active_run;
}

// ═══════════════════════════════════════════════════════════════════════
// Test 2: Production catalog includes both Main and SubAgent scopes
// ═══════════════════════════════════════════════════════════════════════

/// Verifies that `wire_builtin_catalog_execution` registers tools in both
/// `main` and `sub-agent` scopes, and that the sub-agent scope has a
/// restricted profile (no agent-dispatch tools like TaskCreate).
#[tokio::test]
async fn production_catalog_has_both_main_and_sub_agent_scopes() {
    let (_temp, workspace) = temp_workspace();
    let task_wiring = task::wire_task();
    let tools_wiring = tools::composition::wire_builtin_catalog_execution(
        task_wiring.access(),
        noop_memory_source(),
        workspace.control(),
    )
    .expect("wire_builtin_catalog_execution");

    let catalog = tools_wiring.catalog();

    // Main scope: full capabilities, includes agent-dispatch tools.
    let main_snapshot = catalog
        .snapshot(
            &tools::RegistryScopeName::new("main"),
            &tools::ToolProfileName::new("main-full"),
        )
        .expect("main snapshot");
    let main_names: Vec<&str> = main_snapshot
        .tools
        .iter()
        .map(|t| t.name.as_str())
        .collect();
    // TaskCreate (Main-only, Caps::TaskMutation) is an agent-dispatch tool
    // that must appear in the main scope.
    assert!(
        main_names.contains(&"TaskCreate"),
        "main scope must include TaskCreate (agent-dispatch), got: {main_names:?}"
    );

    // Sub-agent scope: restricted, excludes agent-dispatch.
    let sub_snapshot = catalog
        .snapshot(
            &tools::RegistryScopeName::new("sub-agent"),
            &tools::ToolProfileName::new("sub-agent-restricted"),
        )
        .expect("sub-agent snapshot");
    let sub_names: Vec<&str> = sub_snapshot.tools.iter().map(|t| t.name.as_str()).collect();

    assert!(
        !sub_names.contains(&"TaskCreate"),
        "sub-agent scope must exclude TaskCreate, got: {sub_names:?}"
    );
    assert!(
        sub_names.contains(&"Bash"),
        "sub-agent scope must include Bash"
    );
    assert!(
        sub_names.contains(&"Read"),
        "sub-agent scope must include Read"
    );
}
