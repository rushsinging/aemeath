//! AgentClientImpl / RuntimeHandle 结构体定义与公共访问器。

use std::collections::HashMap;
use std::sync::Arc;

use crate::application::loop_engine::chat::ChatEventSinkHandle;
use crate::application::run::context::ParentRunContextSource;
use crate::application::run::context_factory::RuntimeContextFactory;
use sdk::ChatEvent;
use share::config::models::ResolvedModel;
use share::config::MemoryConfig;
use tools::AgentRunner;

pub(crate) type InputPortFactory = dyn Fn(
        Option<Arc<dyn sdk::QueueDrainPort>>,
        Option<Arc<dyn sdk::ChatInputEventPort>>,
    ) -> InputPortPair
    + Send
    + Sync;

/// #1381: Composition-injected input port pair.
/// Application receives this without importing adapter modules.
pub struct InputPortPair {
    pub queue: crate::adapters::input_buffer::RuntimeQueueDrainPort,
    pub input_events: crate::adapters::input_buffer::RuntimeInputEventDrainPort,
}

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
/// #1385: Separated from per-Run [`RuntimeContext`] so that each Run gets its own
/// frozen provider binding, cancellation scope, and bound context/memory ports
/// via [`RuntimeContextFactory::create`] (held in `runtime_context_factory`).
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
        Arc<std::sync::RwLock<crate::application::run::preparation::SessionState>>,
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

    // ── Skills ──
    pub skills_map: HashMap<String, sdk::SkillView>,

    // ── Config values ──
    pub memory_config: MemoryConfig,
    pub context_size: usize,
    pub language: String,
    pub allow_all: bool,
    /// #1385 Task 7: verbose 标记，从 `ChatRuntimeContext` 迁移至 shell。
    pub verbose: bool,
    /// #1385 Task 7: resume session-id，从 `ChatRuntimeContext` 迁移至 shell。
    pub resume: Option<String>,
    /// 启动 `--resume` 已完成的单次恢复投影；供 Composition/TUI 初始化历史。
    pub startup_resume: Option<sdk::SessionResumeView>,

    // ── Cross-run shared resources ──
    pub(crate) agent_runner: Arc<dyn AgentRunner>,
    /// #1385 Task 6: shared parent context source — written by the Main Run
    /// loop before tool execution, read by sub-agent derivation.
    pub(crate) parent_context_source: ParentRunContextSource,
    pub(crate) tool_result_materializer:
        Arc<crate::application::tool::result_materialization::ToolResultMaterializer>,
    pub(crate) active_run: Arc<crate::application::run::active_registry::ActiveRunRegistry>,
    pub(crate) interaction_bridge: Arc<crate::application::interaction::port::InteractionBridge>,
    pub(crate) session_ingress: Arc<crate::application::session::ingress::SessionIngress>,

    // ── Event/Input factories ──
    pub(crate) event_sink_factory: Arc<
        dyn Fn(tokio::sync::mpsc::UnboundedSender<ChatEvent>) -> ChatEventSinkHandle + Send + Sync,
    >,
    pub(crate) input_port_factory: Arc<InputPortFactory>,

    // ── Session reminders ──
    pub session_reminders: std::sync::Arc<std::sync::RwLock<share::memory::SessionReminders>>,

    // ── Parent capability ports (cloned into per-Run RuntimeContext) ──
    // #1248 Task 3: These ports are held in RuntimeContextFactory → RuntimeServices.
    // Access them via shell.runtime_context_factory.services() rather than
    // duplicating the Arc references here.
    //
    // tool_catalog, tool_execution, and hook_runner are still exposed through
    // tui_launch_context via factory services accessors.

    // ── #1248 Task 3: RuntimeContextFactory (constructed once from static ports) ──
    pub(crate) runtime_context_factory: Arc<RuntimeContextFactory>,
}

impl SessionRuntime {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        session_state: Arc<std::sync::RwLock<crate::application::run::preparation::SessionState>>,
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
        skills_map: HashMap<String, sdk::SkillView>,
        memory_config: MemoryConfig,
        context_size: usize,
        language: String,
        allow_all: bool,
        verbose: bool,
        resume: Option<String>,
        startup_resume: Option<sdk::SessionResumeView>,
        agent_runner: Arc<dyn AgentRunner>,
        parent_context_source: ParentRunContextSource,
        tool_result_materializer: Arc<
            crate::application::tool::result_materialization::ToolResultMaterializer,
        >,
        active_run: Arc<crate::application::run::active_registry::ActiveRunRegistry>,
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
            skills_map,
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
            event_sink_factory: Arc::new(|tx| {
                crate::application::loop_engine::chat::ChatEventSinkHandle::new(
                    crate::adapters::sdk_event_sink::SdkChatEventSink::new(tx),
                )
            }),
            input_port_factory: Arc::new(|queue, events| InputPortPair {
                queue: crate::adapters::input_buffer::RuntimeQueueDrainPort::new(queue),
                input_events: crate::adapters::input_buffer::RuntimeInputEventDrainPort::new(
                    events,
                ),
            }),
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

    pub(crate) fn session_snapshot(&self) -> crate::application::run::preparation::SessionSnapshot {
        self.session_state.read().unwrap().snapshot_for_run()
    }
}

/// Error returned when RuntimeContext assembly fails.
///
/// Merged from `client::RuntimeContextAssemblyError` and
/// `runtime_context_factory::RuntimeContextAssemblyError` (no duplicate naming).
///
/// #1248 Task 3: Typed error variants.  Role/model errors are retained as typed;
/// provider build errors have their own variant.  `SubDerivationFailed` is
/// reserved for truly untyped derivation failures (e.g. spec derivation
/// errors from the domain layer or tool catalog snapshot errors).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeContextAssemblyError {
    /// The RunSpec requests `ParentMediated` interaction but no parent
    /// RuntimeContext was provided.
    #[error("interaction is unavailable — ParentMediated requires a parent")]
    InteractionUnavailable,
    /// The RunSpec requests `BoundaryOnly` hooks but no parent
    /// RuntimeContext was provided.
    #[error("hooks are unavailable — BoundaryOnly requires a parent")]
    HookUnavailable,
    /// The RunSpec requests `Inherit` reasoning but no parent
    /// RuntimeContext was provided.  Inherit without a parent is a
    /// hard error — no fallback to NoOp.
    #[error("reasoning is unavailable — Inherit requires a parent")]
    ReasoningUnavailable,
    /// #1385 Task 6: sub-agent role not found in config.
    #[error("sub-agent role `{role}` not found in config")]
    SubRoleNotFound { role: String },
    /// #1385 Task 6: sub-agent role is disabled.
    #[error("sub-agent role `{role}` is disabled")]
    SubRoleDisabled { role: String },
    /// #1385 Task 6: sub-agent role has no configured model.
    #[error("sub-agent role `{role}` has no configured model")]
    SubRoleNoModel { role: String },
    /// #1385 Task 6: unknown model for sub-agent role.
    #[error("unknown model `{model}` for sub-agent role `{role}`")]
    SubUnknownModel { model: String, role: String },
    /// #1248 Task 3: provider factory failed to build a binding for the
    /// sub-agent role.
    #[error("provider build failed for sub-agent role `{role}`: {message}")]
    SubProviderBuildFailed { role: String, message: String },
    /// #1385 Task 6: derivation guard failed (catch-all for spec/tool-catalog
    /// errors; role/model errors have their own typed variants above).
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

    pub fn tui_launch_context(&self) -> crate::adapters::tui_launch::TuiLaunchContext {
        let shell = &self.inner.shell;
        let services = shell.runtime_context_factory.services();
        let session_snapshot = shell.session_snapshot();
        let resolved_model = shell.model_state.resolved();
        crate::adapters::tui_launch::TuiLaunchContext {
            session_id: session_snapshot.session_id().to_string(),
            startup_resume: shell.startup_resume.clone(),
            model_display: super::mapping::model_display(
                &resolved_model.source_key,
                &resolved_model.model.name,
                &resolved_model.model.id,
            ),
            binding: shell.model_state.binding(),
            tool_catalog: services.tool_catalog.clone(),
            tool_execution: services.tool_execution.clone(),
            system_blocks: shell.system_blocks.clone(),
            system_prompt_text: shell.system_prompt_text.clone(),
            initial_git_context: shell.initial_git_context.clone(),
            user_context: shell.user_context.clone(),
            context_size: shell.context_size,
            verbose: shell.verbose,
            config_view: super::mapping::config_snapshot_to_sdk(session_snapshot.config()),
            agent_runner: shell.agent_runner.clone(),
            allow_all: shell.allow_all,
            max_tool_concurrency: shell.max_tool_concurrency,
            max_agent_concurrency: shell.max_agent_concurrency,
            agent_semaphore: shell.agent_semaphore.clone(),
            memory_config: super::mapping::memory_config_to_sdk(shell.memory_config.clone()),
            skills_map: shell.skills_map.clone(),
            hook_runner: services.hooks.clone(),
            session_reminders: Arc::new(std::sync::Mutex::new(tools::SessionReminders::new())),
            workspace_root: session_snapshot.workspace_root().to_path_buf(),
        }
    }
}
