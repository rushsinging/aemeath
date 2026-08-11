//! AgentClientImpl / RuntimeHandle 结构体定义与公共访问器。

use std::sync::Arc;

use crate::application::loop_engine::chat::ChatEventSinkHandle;
use crate::application::loop_engine::input_strategy::SessionInputPort;
use crate::application::run::context::ParentRunContextSource;
use crate::application::run::context_factory::RuntimeContextFactory;
use sdk::ChatEvent;
use share::config::models::ResolvedModel;
use share::config::MemoryConfig;
use tools::AgentRunner;

trait DynSessionInput: Send + Sync {
    fn defer(&self, event: sdk::ChatInputEvent);
    fn drain_input_events<'a>(
        &'a self,
    ) -> crate::application::loop_engine::chat::InputEventFuture<'a>;
    fn recv_next_input<'a>(
        &'a self,
    ) -> crate::application::loop_engine::chat::InputEventOptFuture<'a>;
}

impl<T> DynSessionInput for T
where
    T: SessionInputPort + Send + Sync,
{
    fn defer(&self, event: sdk::ChatInputEvent) {
        SessionInputPort::defer(self, event);
    }

    fn drain_input_events<'a>(
        &'a self,
    ) -> crate::application::loop_engine::chat::InputEventFuture<'a> {
        crate::application::loop_engine::chat::InputEventDrainPort::drain_input_events(self)
    }

    fn recv_next_input<'a>(
        &'a self,
    ) -> crate::application::loop_engine::chat::InputEventOptFuture<'a> {
        crate::application::loop_engine::chat::InputEventDrainPort::recv_next_input(self)
    }
}

#[derive(Clone)]
pub(crate) struct SessionInputHandle {
    inner: Arc<dyn DynSessionInput>,
}

impl SessionInputHandle {
    pub(crate) fn new<T>(input: T) -> Self
    where
        T: SessionInputPort + Send + Sync,
    {
        Self {
            inner: Arc::new(input),
        }
    }
}

impl crate::application::loop_engine::chat::InputEventDrainPort for SessionInputHandle {
    fn drain_input_events<'a>(
        &'a self,
    ) -> crate::application::loop_engine::chat::InputEventFuture<'a> {
        self.inner.drain_input_events()
    }

    fn recv_next_input<'a>(
        &'a self,
    ) -> crate::application::loop_engine::chat::InputEventOptFuture<'a> {
        self.inner.recv_next_input()
    }
}

impl SessionInputPort for SessionInputHandle {
    fn defer(&self, event: sdk::ChatInputEvent) {
        self.inner.defer(event);
    }
}

pub(crate) type InputPortFactory =
    dyn Fn(Arc<dyn sdk::ChatInputEventPort>) -> SessionInputHandle + Send + Sync;

pub(crate) type EventSinkFactory =
    dyn Fn(tokio::sync::mpsc::UnboundedSender<ChatEvent>) -> ChatEventSinkHandle + Send + Sync;

#[derive(Clone)]
pub struct SessionModelState {
    resolved: Arc<std::sync::RwLock<ResolvedModel>>,
    binding: Arc<std::sync::RwLock<Arc<crate::ports::ProviderBinding>>>,
}

impl SessionModelState {
    pub(crate) fn new(
        resolved: ResolvedModel,
        binding: Arc<crate::ports::ProviderBinding>,
    ) -> Self {
        Self {
            resolved: Arc::new(std::sync::RwLock::new(resolved)),
            binding: Arc::new(std::sync::RwLock::new(binding)),
        }
    }

    pub(crate) fn resolved(&self) -> ResolvedModel {
        self.resolved.read().unwrap().clone()
    }

    pub(crate) fn binding(&self) -> Arc<crate::ports::ProviderBinding> {
        self.binding.read().unwrap().clone()
    }

    pub(crate) fn update_binding(&self, binding: Arc<crate::ports::ProviderBinding>) {
        *self.binding.write().unwrap() = binding;
    }
}

// ─── SessionRuntime — session-level state per §2.2 ───

/// Session-level runtime container that holds state live across all runs.
///
/// Session state is separated from per-Run [`RuntimeContext`] so that each Run
/// gets its own frozen provider binding, cancellation scope, and bound
/// context/memory ports through [`crate::application::run::factory::RunFactory::create`], which
/// delegates Context assembly to the held `runtime_context_factory`.
///
/// Fields are grouped by §2.2 categories:
/// - Session identity, wiring, workspace
/// - Config query/writer, session management
/// - Model switching (provider factory, current binding)
/// - Prompt bootstrap (system blocks, git context, user guidance, skills)
/// - Agent infrastructure (runner, semaphore, materializer, concurrency)
/// - Parent capability ports (shared Arc, cloned into per-Run RuntimeContext)
#[derive(Clone)]
pub struct SessionRuntime {
    // ── Session identity & workspace ──
    pub(crate) session_state:
        Arc<std::sync::RwLock<crate::application::run::creation::SessionState>>,
    pub workspace: project::WorkspaceViews,
    pub wiring: Arc<context::MainSessionWiring>,

    // ── Config ──
    pub config_query: Arc<dyn config::ConfigQuery>,
    pub config_writer: Arc<dyn config::ConfigWriter>,
    pub session_management: Arc<dyn context::SessionManagementPort>,

    // ── Model switching ──
    pub(crate) provider_factory: Arc<dyn crate::ports::ProviderFactory>,
    pub(crate) model_state: SessionModelState,

    // ── Concurrency ──
    pub max_tool_concurrency: usize,
    pub max_agent_concurrency: usize,
    pub(crate) agent_semaphore: Arc<tokio::sync::Semaphore>,

    // ── Prompt bootstrap (static, session-life) ──
    pub system_blocks: Vec<provider::RequestSystemBlock>,
    pub system_prompt_text: String,
    pub initial_git_context: String,
    pub user_context: String,
    pub prompt_model_id: String,

    // ── Skills ──
    pub skill_catalog: Arc<dyn tools::SkillCatalogPort>,
    pub initial_skill_snapshot: tools::SkillCatalogSnapshot,

    // ── Config values ──
    pub memory_config: MemoryConfig,
    pub context_size: usize,
    pub language: String,
    pub allow_all: bool,
    /// Session-owned verbose flag.
    pub verbose: bool,
    /// Session resume identifier.
    pub resume: Option<String>,
    /// 启动 `--resume` 已完成的单次恢复投影；供 Composition/TUI 初始化历史。
    pub startup_resume: Option<sdk::LocalSessionResumeBacking>,

    // ── Cross-run shared resources ──
    pub(crate) agent_runner: Arc<dyn AgentRunner>,
    /// #1385 Task 6: shared parent context source — written by the Main Run
    /// loop before tool execution, read by sub-agent derivation.
    pub(crate) parent_context_source: ParentRunContextSource,
    pub(crate) tool_result_materializer:
        Arc<crate::application::tool::tool_result_materializer::ToolResultMaterializer>,
    pub(crate) active_run: Arc<crate::application::run::active_registry::ActiveRunRegistry>,
    pub(crate) interaction_bridge: Arc<crate::application::interaction::port::InteractionBridge>,
    pub(crate) session_ingress: Arc<crate::application::session::ingress::SessionIngress>,

    // ── Event/Input factories ──
    pub(crate) event_sink_factory: Arc<EventSinkFactory>,
    pub(crate) input_port_factory: Arc<InputPortFactory>,

    // ── Session reminders ──
    pub session_reminders: std::sync::Arc<std::sync::RwLock<share::memory::SessionReminders>>,

    // ── Parent capability ports (cloned into per-Run RuntimeContext) ──
    // #1248 Task 3: These ports are held in RuntimeContextFactory → RuntimeServices.
    // Access them via shell.runtime_context_factory.services() rather than
    // duplicating the Arc references here.
    //
    // Tool, Hook and other per-Run services remain accessible only through
    // RuntimeContextFactory during Run creation.

    // ── #1248 Task 3: RuntimeContextFactory (constructed once from static ports) ──
    pub(crate) runtime_context_factory: Arc<RuntimeContextFactory>,
}

impl SessionRuntime {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        session_state: Arc<std::sync::RwLock<crate::application::run::creation::SessionState>>,
        workspace: project::WorkspaceViews,
        wiring: Arc<context::MainSessionWiring>,
        config_query: Arc<dyn config::ConfigQuery>,
        config_writer: Arc<dyn config::ConfigWriter>,
        session_management: Arc<dyn context::SessionManagementPort>,
        provider_factory: Arc<dyn crate::ports::ProviderFactory>,
        model_state: SessionModelState,
        max_tool_concurrency: usize,
        max_agent_concurrency: usize,
        agent_semaphore: Arc<tokio::sync::Semaphore>,
        system_blocks: Vec<provider::RequestSystemBlock>,
        system_prompt_text: String,
        initial_git_context: String,
        user_context: String,
        prompt_model_id: String,
        skill_catalog: Arc<dyn tools::SkillCatalogPort>,
        initial_skill_snapshot: tools::SkillCatalogSnapshot,
        memory_config: MemoryConfig,
        context_size: usize,
        language: String,
        allow_all: bool,
        verbose: bool,
        resume: Option<String>,
        startup_resume: Option<sdk::LocalSessionResumeBacking>,
        agent_runner: Arc<dyn AgentRunner>,
        parent_context_source: ParentRunContextSource,
        tool_result_materializer: Arc<
            crate::application::tool::tool_result_materializer::ToolResultMaterializer,
        >,
        active_run: Arc<crate::application::run::active_registry::ActiveRunRegistry>,
        event_sink_factory: Arc<EventSinkFactory>,
        input_port_factory: Arc<InputPortFactory>,
        runtime_context_factory: Arc<RuntimeContextFactory>,
    ) -> Self {
        let interaction_bridge =
            Arc::new(crate::application::interaction::port::InteractionBridge::new());
        let session_ingress = Arc::new(crate::application::session::ingress::SessionIngress::new(
            interaction_bridge.clone(),
        ));
        Self {
            session_state,
            workspace,
            wiring,
            config_query,
            config_writer,
            session_management,
            provider_factory,
            model_state,
            max_tool_concurrency,
            max_agent_concurrency,
            agent_semaphore,
            system_blocks,
            system_prompt_text,
            initial_git_context,
            user_context,
            prompt_model_id,
            skill_catalog,
            initial_skill_snapshot,
            memory_config,
            context_size,
            language,
            allow_all,
            verbose,
            resume,
            startup_resume,
            agent_runner,
            parent_context_source,
            tool_result_materializer,
            active_run,
            interaction_bridge,
            session_ingress,
            event_sink_factory,
            input_port_factory,
            session_reminders: Arc::new(std::sync::RwLock::new(
                share::memory::SessionReminders::new(),
            )),
            runtime_context_factory,
        }
    }

    #[cfg(test)]
    pub(crate) fn set_test_session_id(&self, session_id: impl Into<String>) {
        let mut state = self.session_state.write().unwrap();
        let config = state.snapshot_for_run().config().clone();
        state.update_session(session_id, config);
    }

    pub(crate) fn session_snapshot(&self) -> crate::application::run::creation::SessionSnapshot {
        self.session_state.read().unwrap().snapshot_for_run()
    }
}

/// 子 Run 派生 façade 的错误。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeContextAssemblyError {
    #[error("sub-agent role `{role}` not found in config")]
    SubRoleNotFound { role: String },
    #[error("sub derivation failed: {reason}")]
    SubDerivationFailed { reason: String },
}

// ─── 结构体定义 ───

/// AgentClient 的 runtime 实现。
///
/// 持有全部运行时状态（LLM client、tool registry、session 等），
/// CLI 通过 sdk::AgentClient trait 与之交互。
#[derive(Clone)]
pub struct AgentClientImpl {
    pub(crate) inner: Arc<RuntimeHandle>,
}

/// Runtime 内部状态。
///
/// #1385 Task 4-7: `shell` is the single session-level state source.
/// All fields formerly duplicated here have been removed.
pub struct RuntimeHandle {
    pub shell: SessionRuntime,
}

// ─── 公共访问器（CLI runtime.rs 需要） ───

impl AgentClientImpl {
    pub fn session_id(&self) -> String {
        self.inner.shell.session_snapshot().session_id().to_string()
    }

    pub fn cwd(&self) -> std::path::PathBuf {
        self.inner
            .shell
            .session_snapshot()
            .workspace_root()
            .to_path_buf()
    }

    pub fn resolved_model(&self) -> ResolvedModel {
        self.inner.shell.model_state.resolved()
    }

    /// Returns the session shell (session-level state) — #1385.
    pub fn shell(&self) -> &SessionRuntime {
        &self.inner.shell
    }

    pub fn max_tool_concurrency(&self) -> usize {
        self.inner.shell.max_tool_concurrency
    }

    pub fn max_agent_concurrency(&self) -> usize {
        self.inner.shell.max_agent_concurrency
    }

    pub fn startup_snapshot(&self) -> sdk::TuiLaunchContext {
        let shell = &self.inner.shell;
        let session_snapshot = shell.session_snapshot();
        let resolved_model = shell.model_state.resolved();
        sdk::TuiLaunchContext {
            session_id: session_snapshot.session_id().to_string(),
            cwd: session_snapshot.workspace_root().to_path_buf(),
            model_display: super::mapping::model_display(
                &resolved_model.source_key,
                &resolved_model.model.name,
                &resolved_model.model.id,
            ),
            memory_config: super::mapping::memory_config_to_sdk(shell.memory_config.clone()),
            skill_snapshot: super::mapping::skill_snapshot_to_sdk(
                shell.initial_skill_snapshot.clone(),
            ),
            initial_resume_id: shell.resume.clone(),
        }
    }

    pub fn startup_resume(&self) -> Option<sdk::LocalSessionResumeBacking> {
        self.inner.shell.startup_resume.clone()
    }

    pub fn allow_all(&self) -> bool {
        self.inner.shell.allow_all
    }

    pub fn context_size(&self) -> usize {
        self.inner.shell.context_size
    }

    pub fn requested_reasoning(&self) -> provider::ReasoningLevel {
        self.inner.shell.model_state.binding().requested_reasoning
    }
}
