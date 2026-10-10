#[cfg(test)]
#[path = "context_tests.rs"]
mod tests;

use crate::domain::CatalogQuery;
use crate::domain::{
    AgentDispatch, AgentProgressEvent, RegistryScopeName, SkillLoadScope, SkillLoadStatePort,
    SkillQuerySnapshot, ToolProfileName, ToolProgressEvent,
};
use async_trait::async_trait;
use project::WorkspaceReader;
use share::session_types::WorkspaceId;
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
    fn emit_tool_stream(&self, event: ToolProgressEvent);
}
pub trait ReadSet: Send + Sync {
    fn record(&self, path: &str);
    fn contains(&self, path: &str) -> bool;
}
pub trait Guidance: Send + Sync {
    fn language(&self) -> &str;
}

// 授权上下文已下沉 share 共享词汇（policy domain 依赖 share 而非本 crate）；
// 此处 re-export 保持 `crate::domain::AuthorizationContext` 路径兼容。
pub use share::tools_vocab::AuthorizationContext;

/// Read-only workspace capability available to every tool invocation.
#[derive(Clone)]
pub struct WorkspaceReadAccess {
    read: Arc<dyn WorkspaceReader>,
}
impl WorkspaceReadAccess {
    pub fn new(read: Arc<dyn WorkspaceReader>) -> Self {
        Self { read }
    }
    pub fn read(&self) -> Arc<dyn WorkspaceReader> {
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
    memory: Arc<dyn memory::api::MemoryPort>,
    parent_session_id: Option<String>,
    guidance: Arc<dyn Guidance>,
    user_agent: String,
    authorization: AuthorizationContext,
    skill_query: SkillQuerySnapshot,
    skill_load_scope: Option<SkillLoadScope>,
    skill_load_state: Option<Arc<dyn SkillLoadStatePort>>,
    scoring: Option<Arc<dyn systemone::ScoringPort>>,
    selection: share::config::ToolSelection,
}
impl ToolExecutionPorts {
    pub fn new(
        cancellation: Arc<dyn CancellationSignal>,
        workspace: WorkspaceReadAccess,
        read_set: Arc<dyn ReadSet>,
        memory: Arc<dyn memory::api::MemoryPort>,
        guidance: Arc<dyn Guidance>,
    ) -> Self {
        Self {
            agent: None,
            catalog: None,
            cancellation,
            progress: None,
            workspace,
            read_set,
            memory,
            parent_session_id: None,
            guidance,
            user_agent: share::config::Config::default().api.user_agent,
            authorization: AuthorizationContext::STANDARD,
            skill_query: SkillQuerySnapshot::default(),
            skill_load_scope: None,
            skill_load_state: None,
            scoring: None,
            selection: share::config::ToolSelection::default(),
        }
    }
    pub fn with_agent(mut self, agent: Option<Arc<dyn AgentDispatch>>) -> Self {
        self.agent = agent;
        self
    }
    /// 注入 System One 评分端口（`AEMEATH_SCORING_SKILL_MATCH` 开关开启时由装配层注入）。
    pub fn with_scoring(mut self, scoring: Option<Arc<dyn systemone::ScoringPort>>) -> Self {
        self.scoring = scoring;
        self
    }
    /// System One 评分端口（未注入即开关关闭，消费点走纯词法路径）。
    pub fn scoring(&self) -> Option<Arc<dyn systemone::ScoringPort>> {
        self.scoring.clone()
    }
    pub fn with_catalog(mut self, catalog: Option<Arc<dyn CatalogQuery>>) -> Self {
        self.catalog = catalog;
        self
    }
    pub fn with_progress(mut self, progress: Option<Arc<dyn ProgressSink>>) -> Self {
        self.progress = progress;
        self
    }
    pub fn with_user_agent(mut self, user_agent: impl Into<String>) -> Self {
        self.user_agent = user_agent.into();
        self
    }

    pub fn with_skill_query(mut self, skill_query: SkillQuerySnapshot) -> Self {
        self.skill_query = skill_query;
        self
    }

    pub fn with_skill_load_state(
        mut self,
        scope: SkillLoadScope,
        state: Arc<dyn SkillLoadStatePort>,
    ) -> Self {
        self.skill_load_scope = Some(scope);
        self.skill_load_state = Some(state);
        self
    }

    pub fn with_selection(mut self, selection: share::config::ToolSelection) -> Self {
        self.selection = selection;
        self
    }

    pub fn with_memory_context(mut self, parent_session_id: Option<String>) -> Self {
        self.parent_session_id = parent_session_id;
        self
    }
}
#[derive(Clone)]
pub struct ToolExecutionContext {
    scope: ExecutionScope,
    ports: ToolExecutionPorts,
    /// 输出直绑任务日志文件路径（#1890）：per-call 注入（派发点 clone
    /// 本 context 后设置）；工具自声明 `background_log_direct` 且会话
    /// 启用后台化时由 runtime 提供，其余为 None（行为不变）。
    background_log_path: Option<std::path::PathBuf>,
}
impl ToolExecutionContext {
    pub fn new(scope: ExecutionScope, ports: ToolExecutionPorts) -> Self {
        Self {
            scope,
            ports,
            background_log_path: None,
        }
    }

    /// 输出直绑路径注入（#1890）：per-call clone 后调用。
    pub fn with_background_log_path(mut self, path: std::path::PathBuf) -> Self {
        self.background_log_path = Some(path);
        self
    }

    /// 任务日志文件路径（#1890 输出直绑）；None 表示本次调用不直绑。
    pub fn background_log_path(&self) -> Option<&std::path::Path> {
        self.background_log_path.as_deref()
    }
    pub fn selection(&self) -> &share::config::ToolSelection {
        &self.ports.selection
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
    /// System One 评分端口委托（skill_match 开关关闭时为 None）。
    pub fn scoring(&self) -> Option<Arc<dyn systemone::ScoringPort>> {
        self.ports.scoring()
    }
    pub fn cancellation(&self) -> Arc<dyn CancellationSignal> {
        self.ports.cancellation.clone()
    }
    pub fn progress_sink(&self) -> Option<Arc<dyn ProgressSink>> {
        self.ports.progress.clone()
    }
    pub fn workspace_read(&self) -> Arc<dyn WorkspaceReader> {
        self.ports.workspace.read()
    }
    pub fn read_set(&self) -> Arc<dyn ReadSet> {
        self.ports.read_set.clone()
    }
    pub fn memory(&self) -> Arc<dyn memory::api::MemoryPort> {
        self.ports.memory.clone()
    }
    pub fn skill_query(&self) -> &SkillQuerySnapshot {
        &self.ports.skill_query
    }
    pub fn skill_load_scope(&self) -> Option<&SkillLoadScope> {
        self.ports.skill_load_scope.as_ref()
    }
    pub fn skill_load_state(&self) -> Option<Arc<dyn SkillLoadStatePort>> {
        self.ports.skill_load_state.clone()
    }
    pub fn parent_session_id(&self) -> Option<String> {
        self.ports.parent_session_id.clone()
    }
    pub fn guidance(&self) -> Arc<dyn Guidance> {
        self.ports.guidance.clone()
    }
    pub fn user_agent(&self) -> &str {
        &self.ports.user_agent
    }
    pub fn authorization(&self) -> AuthorizationContext {
        self.ports.authorization
    }
    pub fn with_cancellation(&self, cancellation: Arc<dyn CancellationSignal>) -> Self {
        let mut next = self.clone();
        next.ports.cancellation = cancellation;
        next
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

    pub fn with_skill_load_state(
        &self,
        scope: SkillLoadScope,
        state: Arc<dyn SkillLoadStatePort>,
    ) -> Self {
        let mut next = self.clone();
        next.ports.skill_load_scope = Some(scope);
        next.ports.skill_load_state = Some(state);
        next
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
pub struct FixedGuidance {
    pub language: String,
}
impl Guidance for FixedGuidance {
    fn language(&self) -> &str {
        &self.language
    }
}
