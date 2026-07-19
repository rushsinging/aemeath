use std::{
    collections::HashMap,
    future::Future,
    path::PathBuf,
    pin::Pin,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
};

use async_trait::async_trait;
use project::{ProjectIdentity, WorkspaceError, WorkspaceId, WorkspaceRead};
use serde_json::{json, Value};

use super::{
    catalog::{CatalogAdapter, ToolBacking},
    execution::{BoundExecutionContexts, ExecutionAdapter},
    tool_registry::ToolRegistry,
};
use crate::domain::published_language::ToolOutcome as ExecutionOutcome;
use crate::domain::{
    scope_profile::{RegistryScopeBuilder, ToolRegistrationSpec},
    CancellationSignal, ExecutionScope, FixedGuidance, FixedPlanMode, MutexReadSet,
    RegistryScopeName, ToolCapabilities, ToolCatalogPort, ToolExecutionContext, ToolExecutionPort,
    ToolExecutionPorts, ToolInvocation, ToolName, ToolProfile, ToolProfileName, ToolSuspension,
    TypedTool, TypedToolResult, WorkspaceReadAccess,
};

struct ContractPorts {
    catalog: Arc<dyn ToolCatalogPort>,
    execution: Arc<dyn ToolExecutionPort>,
    backing: ToolBacking,
    registry: Arc<ToolRegistry>,
    contexts: Arc<BoundExecutionContexts>,
    scope: ExecutionScope,
    calls: Arc<AtomicUsize>,
}

type FactoryFuture = Pin<Box<dyn Future<Output = ContractPorts>>>;
type ContractFactory = fn() -> FactoryFuture;

fn adapter_factory() -> FactoryFuture {
    Box::pin(async {
        let calls = Arc::new(AtomicUsize::new(0));
        let registry = Arc::new(ToolRegistry::new());
        registry.register(CountingTool {
            calls: calls.clone(),
        });
        registry.register(SuspendingTool);

        let mut scope_builder = RegistryScopeBuilder::new("main");
        for (name, caps) in [
            ("Counting", ToolCapabilities::ReadWorkspace),
            ("Suspend", ToolCapabilities::UserInteraction),
            ("Ghost", ToolCapabilities::empty()),
        ] {
            scope_builder
                .register_mut(ToolRegistrationSpec::new(name, caps))
                .unwrap();
        }
        let mut scopes = HashMap::new();
        scopes.insert(RegistryScopeName::new("main"), scope_builder.build());
        let mut profiles = HashMap::new();
        profiles.insert(
            ToolProfileName::new("full"),
            ToolProfile::baseline(ToolCapabilities::all()),
        );
        profiles.insert(
            ToolProfileName::new("read-only"),
            ToolProfile::derive_restricted(
                profiles.get(&ToolProfileName::new("full")).unwrap(),
                ToolCapabilities::ReadWorkspace,
            )
            .unwrap(),
        );
        profiles.insert(
            ToolProfileName::new("none"),
            ToolProfile::baseline(ToolCapabilities::empty()),
        );
        let backing = ToolBacking::try_new(registry.clone(), scopes, profiles).unwrap();
        let contexts = Arc::new(BoundExecutionContexts::new());
        let context = context_for_profile("full");
        let scope = context.scope().clone();
        contexts.bind(context).expect("bind context");
        contexts
            .bind(context_for_run_and_profile("restricted-run", "read-only"))
            .expect("bind restricted context");
        ContractPorts {
            catalog: Arc::new(CatalogAdapter::new(backing.clone())),
            execution: Arc::new(ExecutionAdapter::new(backing.clone(), contexts.clone())),
            backing,
            registry,
            contexts,
            scope,
            calls,
        }
    })
}

#[tokio::test]
async fn catalog_execution_adapter_satisfies_contract() {
    run_contract(adapter_factory).await;
}

async fn run_contract(factory: ContractFactory) {
    let ports = factory().await;
    let full = ToolProfileName::new("full");
    let none = ToolProfileName::new("none");
    let scope_name = RegistryScopeName::new("main");
    let snapshot = ports.catalog.snapshot(&scope_name, &full).unwrap();
    assert_eq!(snapshot.profile, full);
    assert_eq!(
        snapshot
            .tools
            .iter()
            .map(|tool| tool.name.as_str())
            .collect::<Vec<_>>(),
        vec!["counting", "suspend"]
    );
    assert!(ports
        .catalog
        .snapshot(&scope_name, &none)
        .unwrap()
        .is_empty());
    let read_only = ToolProfileName::new("read-only");
    let restricted = ports.catalog.snapshot(&scope_name, &read_only).unwrap();
    assert_eq!(
        restricted
            .tools
            .iter()
            .map(|tool| tool.name.as_str())
            .collect::<Vec<_>>(),
        vec!["counting"]
    );
    assert!(ports
        .execution
        .execute(
            invocation_for_run_and_profile(
                &ports.scope,
                "restricted-run",
                "Counting",
                json!({"value":"restricted"}),
                "read-only",
            ),
            &ManualCancellation::default(),
        )
        .await
        .is_success());
    let restricted_suspend = ports
        .execution
        .execute(
            invocation_for_run_and_profile(
                &ports.scope,
                "restricted-run",
                "Suspend",
                json!({}),
                "read-only",
            ),
            &ManualCancellation::default(),
        )
        .await;
    assert!(matches!(
        restricted_suspend,
        ExecutionOutcome::Failure(ref failure)
            if failure.kind == crate::domain::published_language::ToolErrorKind::Unauthorized
    ));

    let cancellation = ManualCancellation::default();
    let outcome = ports
        .execution
        .execute(
            invocation(&ports.scope, "unknown", json!({})),
            &cancellation,
        )
        .await;
    assert_unavailable(outcome);
    assert_eq!(ports.calls.load(Ordering::SeqCst), 1);

    // MCP Ready conservative seam: registry-only dynamic registration cannot
    // grant scope membership or profile authorization.
    let dynamic_calls = Arc::new(AtomicUsize::new(0));
    ports
        .backing
        .register_conservative_dynamic(CountingNamedTool {
            name: "Dynamic",
            calls: dynamic_calls.clone(),
        });
    assert!(ports
        .catalog
        .snapshot(&scope_name, &full)
        .unwrap()
        .find(&ToolName::new("Dynamic"))
        .is_none());
    assert_unavailable(
        ports
            .execution
            .execute(
                invocation(&ports.scope, "Dynamic", json!({"value":"x"})),
                &cancellation,
            )
            .await,
    );
    assert_eq!(dynamic_calls.load(Ordering::SeqCst), 0);

    let outcome = ports
        .execution
        .execute(
            invocation_with_profile(&ports.scope, "Counting", json!({"value":"x"}), "none"),
            &cancellation,
        )
        .await;
    assert!(
        matches!(outcome, ExecutionOutcome::Failure(ref f) if f.kind == crate::domain::published_language::ToolErrorKind::Unauthorized)
    );
    assert_eq!(ports.calls.load(Ordering::SeqCst), 1);

    let outcome = ports
        .execution
        .execute(
            invocation(&ports.scope, "Counting", json!({})),
            &cancellation,
        )
        .await;
    assert!(
        matches!(outcome, ExecutionOutcome::Failure(ref f) if f.kind == crate::domain::published_language::ToolErrorKind::InvalidInput)
    );
    assert_eq!(ports.calls.load(Ordering::SeqCst), 1);

    assert!(ports
        .execution
        .execute(
            invocation(&ports.scope, "Counting", json!({"value":"ok"})),
            &cancellation,
        )
        .await
        .is_success());
    assert_eq!(ports.calls.load(Ordering::SeqCst), 2);

    let suspended = ports
        .execution
        .execute(
            invocation(&ports.scope, "Suspend", json!({})),
            &cancellation,
        )
        .await;
    assert!(matches!(
        suspended,
        ExecutionOutcome::Suspended(ToolSuspension::UserInteraction(_))
    ));

    let stale = snapshot;
    assert!(stale.find(&ToolName::new("Counting")).is_some());
    assert!(ports.registry.unregister("Counting"));
    assert_unavailable(
        ports
            .execution
            .execute(
                invocation(&ports.scope, "Counting", json!({"value":"again"})),
                &cancellation,
            )
            .await,
    );
    assert!(ports
        .catalog
        .snapshot(&scope_name, &ToolProfileName::new("full"))
        .unwrap()
        .find(&ToolName::new("Counting"))
        .is_none());
    assert_eq!(ports.calls.load(Ordering::SeqCst), 2);

    ports.contexts.unbind(ports.scope.run_id());
    let cancelled = ManualCancellation::cancelled();
    assert!(ports
        .execution
        .execute(invocation(&ports.scope, "Suspend", json!({})), &cancelled)
        .await
        .is_cancelled());
}

fn invocation(scope: &ExecutionScope, name: &str, input: Value) -> ToolInvocation {
    ToolInvocation::new(name, input, scope.clone())
}

fn invocation_for_run_and_profile(
    scope: &ExecutionScope,
    run_id: &str,
    name: &str,
    input: Value,
    profile: &str,
) -> ToolInvocation {
    let altered = ExecutionScope::builder(
        run_id,
        scope.workspace_id().clone(),
        scope.workspace_root().to_path_buf(),
    )
    .registry_scope(scope.registry_scope().clone())
    .profile(ToolProfileName::new(profile))
    .build();
    ToolInvocation::new(name, input, altered)
}

fn invocation_with_profile(
    scope: &ExecutionScope,
    name: &str,
    input: Value,
    profile: &str,
) -> ToolInvocation {
    let altered = ExecutionScope::builder(
        scope.run_id(),
        scope.workspace_id().clone(),
        scope.workspace_root().to_path_buf(),
    )
    .registry_scope(scope.registry_scope().clone())
    .profile(ToolProfileName::new(profile))
    .build();
    ToolInvocation::new(name, input, altered)
}

fn assert_unavailable(outcome: ExecutionOutcome) {
    assert!(
        matches!(outcome, ExecutionOutcome::Failure(ref f) if f.kind == crate::domain::published_language::ToolErrorKind::ToolUnavailable)
    );
}

struct CountingTool {
    calls: Arc<AtomicUsize>,
}

#[async_trait]
impl TypedTool for CountingTool {
    type Output = Value;
    fn name(&self) -> &str {
        "Counting"
    }
    fn description(&self) -> &str {
        "count calls"
    }
    fn input_schema(&self) -> Value {
        json!({"type":"object","properties":{"value":{"type":"string"}},"required":["value"]})
    }
    async fn call(&self, _input: Value, _ctx: &ToolExecutionContext) -> TypedToolResult<Value> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        TypedToolResult::success("ok", Value::Null)
    }
}

struct CountingNamedTool {
    name: &'static str,
    calls: Arc<AtomicUsize>,
}

#[async_trait]
impl TypedTool for CountingNamedTool {
    type Output = Value;
    fn name(&self) -> &str {
        self.name
    }
    fn description(&self) -> &str {
        "dynamic counting tool"
    }
    fn input_schema(&self) -> Value {
        json!({"type":"object","properties":{"value":{"type":"string"}},"required":["value"]})
    }
    async fn call(&self, _input: Value, _ctx: &ToolExecutionContext) -> TypedToolResult<Value> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        TypedToolResult::success("ok", Value::Null)
    }
}

struct SuspendingTool;
#[async_trait]
impl TypedTool for SuspendingTool {
    type Output = Value;
    fn name(&self) -> &str {
        "Suspend"
    }
    fn description(&self) -> &str {
        "suspend"
    }
    fn input_schema(&self) -> Value {
        json!({"type":"object","properties":{}})
    }
    fn suspension(&self, _input: &Value) -> Option<Result<ToolSuspension, String>> {
        Some(Ok(ToolSuspension::UserInteraction(
            crate::domain::UserInteractionSpec::new(vec![crate::domain::UserQuestion::new(
                "continue?",
                vec![crate::domain::UserOption::title_only("yes")],
                false,
                true,
                None,
            )]),
        )))
    }
    async fn call(&self, _input: Value, _ctx: &ToolExecutionContext) -> TypedToolResult<Value> {
        panic!("suspension seam must bypass call")
    }
}

#[derive(Default)]
struct ManualCancellation(AtomicBool);
impl ManualCancellation {
    fn cancelled() -> Self {
        Self(AtomicBool::new(true))
    }
}
#[async_trait]
impl CancellationSignal for ManualCancellation {
    fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
    async fn cancelled(&self) {
        std::future::pending::<()>().await
    }
    fn child_signal(&self) -> Arc<dyn CancellationSignal> {
        Arc::new(Self(self.0.load(Ordering::SeqCst).into()))
    }
}

struct FakeWorkspace {
    root: PathBuf,
    id: WorkspaceId,
}
impl FakeWorkspace {
    fn new() -> Self {
        Self {
            root: std::env::temp_dir(),
            id: WorkspaceId::from("contract-workspace"),
        }
    }
}
impl WorkspaceRead for FakeWorkspace {
    fn current_workspace_root(&self) -> PathBuf {
        self.root.clone()
    }
    fn workspace_id(&self) -> WorkspaceId {
        self.id.clone()
    }
    fn project_identity(&self) -> ProjectIdentity {
        ProjectIdentity {
            initial_cwd: self.root.display().to_string(),
            git_common_dir: None,
        }
    }
    fn current_path_base(&self) -> PathBuf {
        self.root.clone()
    }
    fn resolve(&self, path: &std::path::Path) -> PathBuf {
        self.root.join(path)
    }
    fn resolve_file_path(&self, path: &std::path::Path) -> Result<PathBuf, WorkspaceError> {
        Ok(self.resolve(path))
    }
    fn resolve_search_path(&self, path: &std::path::Path) -> Result<PathBuf, WorkspaceError> {
        Ok(self.resolve(path))
    }
    fn in_worktree(&self) -> bool {
        false
    }
    fn current_branch(&self) -> Result<Option<String>, WorkspaceError> {
        Ok(None)
    }
    fn initial_cwd(&self) -> PathBuf {
        self.root.clone()
    }
}

fn context_for_profile(profile: &str) -> ToolExecutionContext {
    context_for_run_and_profile("contract-run", profile)
}

fn context_for_run_and_profile(run_id: &str, profile: &str) -> ToolExecutionContext {
    let workspace = Arc::new(FakeWorkspace::new());
    let scope = ExecutionScope::builder(
        run_id,
        workspace.workspace_id(),
        workspace.current_workspace_root(),
    )
    .profile(ToolProfileName::new(profile))
    .build();
    let ports = ToolExecutionPorts::new(
        Arc::new(ManualCancellation::default()),
        WorkspaceReadAccess::new(workspace),
        Arc::new(MutexReadSet(Arc::new(std::sync::Mutex::new(
            Default::default(),
        )))),
        Arc::new(FixedPlanMode(None)),
        Arc::new(memory::NoOpMemory),
        Arc::new(FixedGuidance {
            language: "en".into(),
            allow_all: true,
        }),
    );
    ToolExecutionContext::new(scope, ports)
}
