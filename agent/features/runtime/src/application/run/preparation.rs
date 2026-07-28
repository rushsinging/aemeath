use std::path::{Path, PathBuf};

use share::config::domain::snapshot::ConfigSnapshot;

use crate::application::run::execution_state::RunExecutionState;
use crate::domain::agent_run::{Run, RunId, RunSpec, RunSpecError};

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

    pub(crate) fn with_identity(
        &self,
        session_id: impl Into<String>,
        workspace_root: PathBuf,
        model_key: impl Into<String>,
    ) -> Self {
        self.with_bound_values(session_id, workspace_root, model_key, self.config.clone())
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

/// 子 Run 可见的父能力上限。只携带 identity 与纯值 RunSpec。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParentRunCapabilities {
    run_id: RunId,
    spec: RunSpec,
}

impl ParentRunCapabilities {
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

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RunPreparationError {
    #[error("子 Run 能力不得超过父 Run")]
    CapabilityEscalation,
    #[error("RuntimeContext 装配失败")]
    ContextAssembly,
    #[error("Session 快照已过期")]
    SessionSnapshotStale,
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

impl From<RunSpecError> for RunPreparationError {
    fn from(_: RunSpecError) -> Self {
        Self::CapabilityEscalation
    }
}

/// RuntimeContextFactory 的纯值准备输入。
///
/// 输入消息不属于装配契约；首次和后续输入都必须经 InputPort 激活 Run。
#[derive(Debug, Clone)]
pub struct RunPreparationRequest {
    spec: RunSpec,
    session: SessionSnapshot,
    parent: Option<ParentRunCapabilities>,
}

impl RunPreparationRequest {
    pub fn new(
        spec: RunSpec,
        session: SessionSnapshot,
        parent: Option<ParentRunCapabilities>,
    ) -> Result<Self, RunPreparationError> {
        if let Some(parent) = &parent {
            spec.validate_against(parent.spec())?;
        }
        Ok(Self {
            spec,
            session,
            parent,
        })
    }

    pub fn spec(&self) -> &RunSpec {
        &self.spec
    }

    pub fn session(&self) -> &SessionSnapshot {
        &self.session
    }

    pub fn parent(&self) -> Option<&ParentRunCapabilities> {
        self.parent.as_ref()
    }

    pub(crate) fn with_session(mut self, session: SessionSnapshot) -> Self {
        self.session = session;
        self
    }

    pub(crate) fn into_parts(self) -> (RunSpec, SessionSnapshot, Option<ParentRunCapabilities>) {
        (self.spec, self.session, self.parent)
    }
}

/// Factory 最终准备产物。
///
/// `PreparedRun` 同时持有 Run identity、execution state、session snapshot
/// 与可选 RuntimeContext，避免通过额外 identity copy 丢失领域状态。
pub struct PreparedRun {
    run: Run,
    execution: RunExecutionState,
    session: SessionSnapshot,
    context: Option<crate::application::run::context::RuntimeContext>,
}

impl PreparedRun {
    pub fn idle(spec: RunSpec, parent_run_id: Option<RunId>, session: SessionSnapshot) -> Self {
        Self {
            run: Run::new(spec, parent_run_id),
            execution: RunExecutionState::new(),
            session,
            context: None,
        }
    }

    pub fn from_request(request: RunPreparationRequest) -> Self {
        let (spec, session, parent) = request.into_parts();
        Self::idle(spec, parent.map(|parent| parent.run_id), session)
    }

    pub(crate) fn with_context(
        spec: RunSpec,
        parent_run_id: Option<RunId>,
        session: SessionSnapshot,
        context: crate::application::run::context::RuntimeContext,
    ) -> Self {
        Self {
            run: Run::new(spec, parent_run_id),
            execution: RunExecutionState::new(),
            session,
            context: Some(context),
        }
    }

    pub fn run(&self) -> &Run {
        &self.run
    }

    pub fn execution(&self) -> &RunExecutionState {
        &self.execution
    }

    pub fn session(&self) -> &SessionSnapshot {
        &self.session
    }

    pub fn context(&self) -> Option<&crate::application::run::context::RuntimeContext> {
        self.context.as_ref()
    }

    pub fn into_parts(
        self,
    ) -> (
        Run,
        RunExecutionState,
        SessionSnapshot,
        Option<crate::application::run::context::RuntimeContext>,
    ) {
        (self.run, self.execution, self.session, self.context)
    }
}
