use std::path::{Path, PathBuf};
use std::sync::Arc;

use share::config::domain::snapshot::ConfigSnapshot;

use crate::application::loop_engine::chat::ChatEventSinkHandle;
use crate::application::run::context::RunUsageTracker;
use crate::application::run::execution_state::RunExecutionState;
use crate::application::run::workspace::RuntimeWorkspaceAccess;
use crate::domain::agent_run::{Run, RunId, RunSpec, RunSpecError};

/// 纯值 [`RunCreationRequest`] 的 Session live binding view。
#[derive(Clone)]
pub(crate) struct SessionRunBindings {
    wiring: Arc<context::MainSessionWiring>,
    provider: Arc<crate::ports::ProviderBinding>,
    interaction: Arc<dyn crate::application::interaction::port::InteractionPort>,
    reasoning: Arc<std::sync::Mutex<share::reasoning::ReasoningLevel>>,
    event_sink: ChatEventSinkHandle,
    /// Per-Session usage tracker — shared across all Main Runs in the same
    /// session so a new Run inherits the last known API total tokens instead
    /// of falling back to a heuristic estimate on its first step.
    usage: RunUsageTracker,
}

impl SessionRunBindings {
    pub(crate) fn new(
        wiring: Arc<context::MainSessionWiring>,
        provider: Arc<crate::ports::ProviderBinding>,
        interaction: Arc<dyn crate::application::interaction::port::InteractionPort>,
        reasoning: Arc<std::sync::Mutex<share::reasoning::ReasoningLevel>>,
        event_sink: ChatEventSinkHandle,
        usage: RunUsageTracker,
    ) -> Self {
        Self {
            wiring,
            provider,
            interaction,
            reasoning,
            event_sink,
            usage,
        }
    }

    pub(crate) fn wiring(&self) -> &Arc<context::MainSessionWiring> {
        &self.wiring
    }

    pub(crate) fn provider(&self) -> &Arc<crate::ports::ProviderBinding> {
        &self.provider
    }

    pub(crate) fn interaction(
        &self,
    ) -> &Arc<dyn crate::application::interaction::port::InteractionPort> {
        &self.interaction
    }

    pub(crate) fn reasoning(&self) -> &Arc<std::sync::Mutex<share::reasoning::ReasoningLevel>> {
        &self.reasoning
    }

    pub(crate) fn event_sink(&self) -> &ChatEventSinkHandle {
        &self.event_sink
    }

    pub(crate) fn usage(&self) -> &RunUsageTracker {
        &self.usage
    }
}

/// 会话级可变事实的 owner。
///
/// 不持有 RuntimeServices、RuntimeContext 或 per-Run execution state。
pub struct SessionState {
    session_id: String,
    workspace_root: PathBuf,
    model_key: String,
    config: ConfigSnapshot,
    revision: u64,
}

impl SessionState {
    pub fn new(
        session_id: impl Into<String>,
        workspace_root: PathBuf,
        model_key: impl Into<String>,
        config: ConfigSnapshot,
    ) -> Self {
        Self {
            session_id: session_id.into(),
            workspace_root,
            model_key: model_key.into(),
            config,
            revision: 0,
        }
    }

    pub fn update_session(&mut self, session_id: impl Into<String>, config: ConfigSnapshot) {
        self.session_id = session_id.into();
        self.config = config;
        self.revision = self.revision.saturating_add(1);
    }

    pub fn update_model(&mut self, model_key: impl Into<String>, config: ConfigSnapshot) {
        self.model_key = model_key.into();
        self.config = config;
        self.revision = self.revision.saturating_add(1);
    }

    pub fn update_provider_binding(
        &mut self,
        binding: &crate::ports::ProviderBinding,
        config: ConfigSnapshot,
    ) {
        self.update_model(
            format!("{}/{}", binding.model.provider, binding.model.model),
            config,
        );
    }

    pub fn update_workspace(&mut self, workspace_root: PathBuf) {
        self.workspace_root = workspace_root;
        self.revision = self.revision.saturating_add(1);
    }

    pub fn snapshot_for_run(&self) -> SessionSnapshot {
        SessionSnapshot {
            session_id: self.session_id.clone(),
            workspace_root: self.workspace_root.clone(),
            model_key: self.model_key.clone(),
            config: self.config.clone(),
            revision: self.revision,
        }
    }
}

/// Run 准备时冻结的会话纯值快照。
#[derive(Debug, Clone)]
pub struct SessionSnapshot {
    session_id: String,
    workspace_root: PathBuf,
    model_key: String,
    config: ConfigSnapshot,
    revision: u64,
}

impl SessionSnapshot {
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    pub fn workspace_root(&self) -> &Path {
        &self.workspace_root
    }

    pub fn model_key(&self) -> &str {
        &self.model_key
    }

    pub fn config(&self) -> &ConfigSnapshot {
        &self.config
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub(crate) fn with_bound_values(
        &self,
        session_id: impl Into<String>,
        workspace_root: PathBuf,
        model_key: impl Into<String>,
        config: ConfigSnapshot,
    ) -> Self {
        Self {
            session_id: session_id.into(),
            workspace_root,
            model_key: model_key.into(),
            revision: config.revision().get(),
            config,
        }
    }
}

/// 子 Run 可见的父纯值事实与 capability ceiling。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParentRunFacts {
    run_id: RunId,
    spec: RunSpec,
}

impl ParentRunFacts {
    pub fn new(run_id: RunId, spec: RunSpec) -> Self {
        Self { run_id, spec }
    }

    pub fn run_id(&self) -> &RunId {
        &self.run_id
    }

    pub fn spec(&self) -> &RunSpec {
        &self.spec
    }
}

/// 只在 Factory 装配子 Run 时可见的父 live binding view。
#[derive(Clone)]
pub(crate) struct ParentRunBindings {
    context: Arc<crate::application::run::context::RuntimeContext>,
    workspace: RuntimeWorkspaceAccess,
}

impl ParentRunBindings {
    pub(crate) fn from_active_run(
        context: Arc<crate::application::run::context::RuntimeContext>,
        workspace: RuntimeWorkspaceAccess,
    ) -> Self {
        Self { context, workspace }
    }

    pub(crate) fn context(&self) -> &Arc<crate::application::run::context::RuntimeContext> {
        &self.context
    }

    pub(crate) fn workspace(&self) -> &RuntimeWorkspaceAccess {
        &self.workspace
    }
}

#[derive(Clone)]
pub(crate) enum RunCreationBindings {
    Session(SessionRunBindings),
    Parent(ParentRunBindings),
}

impl RunCreationBindings {
    pub(crate) fn session(&self) -> Option<&SessionRunBindings> {
        match self {
            Self::Session(bindings) => Some(bindings),
            Self::Parent(_) => None,
        }
    }

    pub(crate) fn parent(&self) -> Option<&ParentRunBindings> {
        match self {
            Self::Parent(bindings) => Some(bindings),
            Self::Session(_) => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RunCreationError {
    #[error("子 Run 能力不得超过父 Run")]
    CapabilityEscalation,
    #[error("RuntimeContext 装配失败")]
    ContextAssembly,
    #[error("子 Run 角色不存在：{role}")]
    SubRoleNotFound { role: String },
    #[error("sub-agent role disabled: {role}")]
    SubRoleDisabled { role: String },
    #[error("子 Run 角色未配置模型：{role}")]
    SubRoleNoModel { role: String },
    #[error("子 Run 模型不存在：{model}")]
    SubUnknownModel { model: String },
    #[error("子 Run provider 构造失败：{message}")]
    SubProviderBuild { message: String },
    #[error("子 Run Tool Catalog 构造失败：{message}")]
    SubToolCatalog { message: String },
}

impl From<RunSpecError> for RunCreationError {
    fn from(_: RunSpecError) -> Self {
        Self::CapabilityEscalation
    }
}

/// RuntimeContextFactory 的纯值准备输入。
///
/// 输入消息不属于装配契约；首次和后续输入都必须经 InputPort 激活 Run。
#[derive(Clone)]
pub struct RunCreationRequest {
    run_id: Option<RunId>,
    spec: RunSpec,
    session: SessionSnapshot,
    parent: Option<ParentRunFacts>,
}

impl RunCreationRequest {
    pub fn new(
        spec: RunSpec,
        session: SessionSnapshot,
        parent: Option<ParentRunFacts>,
    ) -> Result<Self, RunCreationError> {
        if let Some(parent) = &parent {
            spec.validate_against(parent.spec())?;
        }
        Ok(Self {
            run_id: None,
            spec,
            session,
            parent,
        })
    }

    pub(crate) fn run_id(&self) -> Option<&RunId> {
        self.run_id.as_ref()
    }

    pub(crate) fn with_run_id(mut self, run_id: RunId) -> Self {
        self.run_id = Some(run_id);
        self
    }

    pub fn spec(&self) -> &RunSpec {
        &self.spec
    }

    pub fn session(&self) -> &SessionSnapshot {
        &self.session
    }

    pub fn parent(&self) -> Option<&ParentRunFacts> {
        self.parent.as_ref()
    }

    pub(crate) fn with_session(mut self, session: SessionSnapshot) -> Self {
        self.session = session;
        self
    }
}

/// 一次完整、可执行的 Run 实例。
///
/// `RunInstance` 统一拥有领域 Run、执行状态、Session 快照与冻结的
/// `RuntimeContext`，调用方不得将这些状态拆散后分别启动。
pub struct RunInstance {
    run: Run,
    execution: RunExecutionState,
    session: SessionSnapshot,
    context: crate::application::run::context::RuntimeContext,
    workspace: Option<crate::application::run::workspace::RuntimeWorkspaceAccess>,
}

impl RunInstance {
    pub(crate) fn new(
        run_id: RunId,
        spec: RunSpec,
        parent_run_id: Option<RunId>,
        session: SessionSnapshot,
        context: crate::application::run::context::RuntimeContext,
        workspace: Option<crate::application::run::workspace::RuntimeWorkspaceAccess>,
    ) -> Self {
        Self {
            run: Run::with_id_and_stop_hook_policy(
                run_id,
                spec,
                parent_run_id,
                context.config().stop_hook_policy(),
            ),
            execution: RunExecutionState::new(),
            session,
            context,
            workspace,
        }
    }

    pub fn run(&self) -> &Run {
        &self.run
    }

    pub fn session(&self) -> &SessionSnapshot {
        &self.session
    }

    pub fn context(&self) -> &crate::application::run::context::RuntimeContext {
        &self.context
    }

    pub fn workspace(&self) -> Option<&crate::application::run::workspace::RuntimeWorkspaceAccess> {
        self.workspace.as_ref()
    }

    pub fn initialize(&mut self, messages: Vec<share::message::Message>, step_count: usize) {
        self.execution.initialize_for_launch(messages, step_count);
    }

    pub(crate) fn execution_parts_mut(
        &mut self,
    ) -> (
        &mut Run,
        &mut RunExecutionState,
        &crate::application::run::context::RuntimeContext,
    ) {
        (&mut self.run, &mut self.execution, &self.context)
    }

    pub(crate) fn execution_mut(&mut self) -> &mut RunExecutionState {
        &mut self.execution
    }
}
