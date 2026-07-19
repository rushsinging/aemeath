#[cfg(test)]
#[path = "context_tests.rs"]
mod tests;

use crate::domain::CatalogQuery;
use crate::domain::{
    AgentDispatch, AgentProgressEvent, RegistryScopeName, SessionReminders, ToolProfileName,
};
use async_trait::async_trait;
use project::{WorkspaceId, WorkspaceRead};
use std::{
    collections::HashSet,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::SystemTime,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InvocationSource {
    #[default]
    MainRun,
    SubAgent,
    Cli,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionScope {
    run_id: String,
    parent_run_id: Option<String>,
    workspace_id: WorkspaceId,
    workspace_root: PathBuf,
    invocation_source: InvocationSource,
    registry_scope: RegistryScopeName,
    profile: ToolProfileName,
    deadline: Option<SystemTime>,
}
pub struct ExecutionScopeBuilder(ExecutionScope);
impl ExecutionScope {
    pub fn builder(
        run_id: impl Into<String>,
        workspace_id: WorkspaceId,
        workspace_root: PathBuf,
    ) -> ExecutionScopeBuilder {
        ExecutionScopeBuilder(Self {
            run_id: run_id.into(),
            parent_run_id: None,
            workspace_id,
            workspace_root,
            invocation_source: InvocationSource::MainRun,
            registry_scope: RegistryScopeName::new("main"),
            profile: ToolProfileName::new("main-full"),
            deadline: None,
        })
    }
    pub fn run_id(&self) -> &str {
        &self.run_id
    }
    pub fn parent_run_id(&self) -> Option<&str> {
        self.parent_run_id.as_deref()
    }
    pub fn workspace_id(&self) -> &WorkspaceId {
        &self.workspace_id
    }
    pub fn workspace_root(&self) -> &std::path::Path {
        &self.workspace_root
    }
    pub fn invocation_source(&self) -> InvocationSource {
        self.invocation_source
    }
    pub fn registry_scope(&self) -> &RegistryScopeName {
        &self.registry_scope
    }
    pub fn profile(&self) -> &ToolProfileName {
        &self.profile
    }
    pub fn deadline(&self) -> Option<SystemTime> {
        self.deadline
    }
}
impl ExecutionScopeBuilder {
    pub fn parent_run_id(mut self, v: impl Into<String>) -> Self {
        self.0.parent_run_id = Some(v.into());
        self
    }
    pub fn invocation_source(mut self, v: InvocationSource) -> Self {
        self.0.invocation_source = v;
        self
    }
    pub fn registry_scope(mut self, v: RegistryScopeName) -> Self {
        self.0.registry_scope = v;
        self
    }
    pub fn profile(mut self, v: ToolProfileName) -> Self {
        self.0.profile = v;
        self
    }
    pub fn deadline(mut self, v: SystemTime) -> Self {
        self.0.deadline = Some(v);
        self
    }
    pub fn build(self) -> ExecutionScope {
        self.0
    }
}
#[async_trait]
pub trait CancellationSignal: Send + Sync {
    fn is_cancelled(&self) -> bool;
    async fn cancelled(&self);
    fn child_signal(&self) -> Arc<dyn CancellationSignal>;
}
pub trait ProgressSink: Send + Sync {
    fn emit(&self, event: AgentProgressEvent);
}
pub trait ReadSet: Send + Sync {
    fn record(&self, path: &str);
    fn contains(&self, path: &str) -> bool;
}
pub trait PlanModeState: Send + Sync {
    fn is_plan_mode(&self) -> Option<bool>;
}
pub trait Guidance: Send + Sync {
    fn language(&self) -> &str;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuthorizationContext {
    pub allow_outside_workspace: bool,
    pub require_read_before_write: bool,
    pub enforce_bash_safety: bool,
    pub enforce_tool_fuse: bool,
    pub enforce_permission_hooks: bool,
}

impl AuthorizationContext {
    pub const STANDARD: Self = Self {
        allow_outside_workspace: false,
        require_read_before_write: true,
        enforce_bash_safety: true,
        enforce_tool_fuse: true,
        enforce_permission_hooks: true,
    };

    pub const ALLOW_ALL: Self = Self {
        allow_outside_workspace: true,
        require_read_before_write: false,
        enforce_bash_safety: false,
        enforce_tool_fuse: false,
        enforce_permission_hooks: false,
    };
}
/// Read-only workspace capability available to every tool invocation.
#[derive(Clone)]
pub struct WorkspaceReadAccess {
    read: Arc<dyn WorkspaceRead>,
}
impl WorkspaceReadAccess {
    pub fn new(read: Arc<dyn WorkspaceRead>) -> Self {
        Self { read }
    }
    pub fn read(&self) -> Arc<dyn WorkspaceRead> {
        self.read.clone()
    }
}
#[derive(Clone)]
pub struct ToolExecutionPorts {
    agent: Option<Arc<dyn AgentDispatch>>,
    catalog: Option<Arc<dyn CatalogQuery>>,
    cancellation: Arc<dyn CancellationSignal>,
    progress: Option<Arc<dyn ProgressSink>>,
    workspace: WorkspaceReadAccess,
    read_set: Arc<dyn ReadSet>,
    plan_mode: Arc<dyn PlanModeState>,
    memory: Arc<dyn memory::MemoryPort>,
    parent_session_id: Option<String>,
    reminders: Option<Arc<Mutex<SessionReminders>>>,
    guidance: Arc<dyn Guidance>,
    authorization: AuthorizationContext,
}
impl ToolExecutionPorts {
    pub fn new(
        cancellation: Arc<dyn CancellationSignal>,
        workspace: WorkspaceReadAccess,
        read_set: Arc<dyn ReadSet>,
        plan_mode: Arc<dyn PlanModeState>,
        memory: Arc<dyn memory::MemoryPort>,
        guidance: Arc<dyn Guidance>,
    ) -> Self {
        Self {
            agent: None,
            catalog: None,
            cancellation,
            progress: None,
            workspace,
            read_set,
            plan_mode,
            memory,
            parent_session_id: None,
            reminders: None,
            guidance,
            authorization: AuthorizationContext::STANDARD,
        }
    }
    pub fn with_agent(mut self, agent: Option<Arc<dyn AgentDispatch>>) -> Self {
        self.agent = agent;
        self
    }
    pub fn with_catalog(mut self, catalog: Option<Arc<dyn CatalogQuery>>) -> Self {
        self.catalog = catalog;
        self
    }
    pub fn with_progress(mut self, progress: Option<Arc<dyn ProgressSink>>) -> Self {
        self.progress = progress;
        self
    }
    pub fn with_memory_context(
        mut self,
        parent_session_id: Option<String>,
        reminders: Option<Arc<Mutex<SessionReminders>>>,
    ) -> Self {
        self.parent_session_id = parent_session_id;
        self.reminders = reminders;
        self
    }
}
#[derive(Clone)]
pub struct ToolExecutionContext {
    scope: ExecutionScope,
    ports: ToolExecutionPorts,
}
impl ToolExecutionContext {
    pub fn new(scope: ExecutionScope, ports: ToolExecutionPorts) -> Self {
        Self { scope, ports }
    }
    pub fn scope(&self) -> &ExecutionScope {
        &self.scope
    }
    pub fn agent_dispatch(&self) -> Option<Arc<dyn AgentDispatch>> {
        self.ports.agent.clone()
    }
    pub fn catalog_query(&self) -> Option<Arc<dyn CatalogQuery>> {
        self.ports.catalog.clone()
    }
    pub fn cancellation(&self) -> Arc<dyn CancellationSignal> {
        self.ports.cancellation.clone()
    }
    pub fn progress_sink(&self) -> Option<Arc<dyn ProgressSink>> {
        self.ports.progress.clone()
    }
    pub fn workspace_read(&self) -> Arc<dyn WorkspaceRead> {
        self.ports.workspace.read()
    }
    pub fn read_set(&self) -> Arc<dyn ReadSet> {
        self.ports.read_set.clone()
    }
    pub fn plan_mode(&self) -> Option<bool> {
        self.ports.plan_mode.is_plan_mode()
    }
    pub fn plan_mode_state(&self) -> Arc<dyn PlanModeState> {
        self.ports.plan_mode.clone()
    }
    pub fn memory(&self) -> Arc<dyn memory::MemoryPort> {
        self.ports.memory.clone()
    }
    pub fn parent_session_id(&self) -> Option<String> {
        self.ports.parent_session_id.clone()
    }
    pub fn session_reminders(&self) -> Option<Arc<Mutex<SessionReminders>>> {
        self.ports.reminders.clone()
    }
    pub fn guidance(&self) -> Arc<dyn Guidance> {
        self.ports.guidance.clone()
    }
    pub fn authorization(&self) -> AuthorizationContext {
        self.ports.authorization
    }
    pub fn with_authorization(&self, authorization: AuthorizationContext) -> Self {
        let mut next = self.clone();
        next.ports.authorization = authorization;
        next
    }
    pub fn with_progress(&self, p: Option<Arc<dyn ProgressSink>>) -> Self {
        let mut n = self.clone();
        n.ports.progress = p;
        n
    }
}
pub struct MutexReadSet(pub Arc<Mutex<HashSet<String>>>);
impl ReadSet for MutexReadSet {
    fn record(&self, p: &str) {
        if let Ok(mut s) = self.0.lock() {
            s.insert(p.into());
        }
    }
    fn contains(&self, p: &str) -> bool {
        self.0.lock().is_ok_and(|s| s.contains(p))
    }
}
pub struct FixedPlanMode(pub Option<bool>);
impl PlanModeState for FixedPlanMode {
    fn is_plan_mode(&self) -> Option<bool> {
        self.0
    }
}
pub struct FixedGuidance {
    pub language: String,
}
impl Guidance for FixedGuidance {
    fn language(&self) -> &str {
        &self.language
    }
}
