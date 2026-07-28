//! AgentClientImpl / RuntimeHandle 结构体定义与公共访问器。

use std::sync::Arc;

use crate::application::main_loop::ChatEventSinkHandle;
use crate::application::runtime_context::ParentRunContextSource;
use crate::application::runtime_context_factory::RuntimeContextFactory;
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

// ─── MainSessionShell — session-level state per §2.2 ───

/// Session-level shell that holds state live across all runs in a Main session.
///
/// #1385: Separated from per-Run [`RuntimeContext`] so that each Run gets its own
/// frozen provider binding, cancellation scope, and bound context/memory ports
/// via [`RuntimeContextFactory::assemble`] (held in `runtime_context_factory`).
///
/// Fields are grouped by §2.2 categories:
/// - Session identity, wiring, workspace
/// - Config query/writer, session management
/// - Model switching (provider factory, current binding)
/// - Prompt bootstrap (system blocks, git context, user guidance, skills)
/// - Agent infrastructure (runner, semaphore, materializer, concurrency)
/// - Parent capability ports (shared Arc, cloned into per-Run RuntimeContext)
#[derive(Clone)]
pub struct MainSessionShell {
    // ── Session identity & workspace ──
    pub session_id: String,
    pub cwd: std::path::PathBuf,
    pub workspace: project::WorkspaceViews,
    pub wiring: Arc<context::MainSessionWiring>,

    // ── Config ──
    pub config_query: Arc<dyn config::ConfigQuery>,
    pub config_writer: Arc<dyn config::ConfigWriter>,
    pub session_management: Arc<dyn context::SessionManagementPort>,

    // ── Model switching ──
    pub(crate) provider_factory: Arc<dyn crate::ports::ProviderFactory>,
    pub resolved_model: ResolvedModel,
    /// Switchable provider binding — model switch updates this,
    /// and the next assembler call freezes the current value.
    pub(crate) current_binding: Arc<std::sync::RwLock<Arc<crate::ports::ProviderBinding>>>,

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
    pub skill_catalog: Arc<dyn tools::SkillCatalogPort>,
    pub initial_skill_snapshot: tools::SkillCatalogSnapshot,

    // ── Config values ──
    pub memory_config: MemoryConfig,
    pub context_size: usize,
    pub language: String,
    pub allow_all: bool,
    /// #1385 Task 7: verbose 标记，从 `ChatRuntimeContext` 迁移至 shell。
    pub verbose: bool,
    /// #1385 Task 7: resume session-id，从 `ChatRuntimeContext` 迁移至 shell。
    pub resume: Option<String>,
    /// 启动 `--resume` 已完成的单次恢复投影；供 Composition/TUI 直接初始化历史。
    pub startup_resume: Option<sdk::SessionResumeView>,

    // ── Cross-run shared resources ──
    pub(crate) agent_runner: Arc<dyn AgentRunner>,
    /// #1385 Task 6: shared parent context source — written by the Main Run
    /// loop before tool execution, read by sub-agent derivation.
    pub(crate) parent_context_source: ParentRunContextSource,
    pub(crate) tool_result_materializer:
        Arc<crate::application::tool_result_materialization::ToolResultMaterializer>,
    pub(crate) active_run: Arc<crate::application::active_run::ActiveRunRegistry>,
    pub(crate) interaction_bridge: Arc<crate::application::interaction::InteractionBridge>,

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
    pub shell: MainSessionShell,
}

// ─── 公共访问器（CLI runtime.rs 需要） ───

impl AgentClientImpl {
    pub fn session_id(&self) -> &str {
        &self.inner.shell.session_id
    }

    pub fn cwd(&self) -> &std::path::Path {
        &self.inner.shell.cwd
    }

    pub fn resolved_model(&self) -> &ResolvedModel {
        &self.inner.shell.resolved_model
    }

    /// Returns the session shell (session-level state) — #1385.
    pub fn shell(&self) -> &MainSessionShell {
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
        crate::adapters::tui_launch::TuiLaunchContext {
            session_id: shell.session_id.clone(),
            startup_resume: shell.startup_resume.clone(),
            model_display: super::mapping::model_display(
                &shell.resolved_model.source_key,
                &shell.resolved_model.model.name,
                &shell.resolved_model.model.id,
            ),
            binding: shell.current_binding.read().unwrap().clone(),
            tool_catalog: services.tool_catalog.clone(),
            tool_execution: services.tool_execution.clone(),
            system_blocks: shell.system_blocks.clone(),
            system_prompt_text: shell.system_prompt_text.clone(),
            initial_git_context: shell.initial_git_context.clone(),
            user_context: shell.user_context.clone(),
            context_size: shell.context_size,
            verbose: shell.verbose,
            config_view: sdk::ConfigView::default(),
            agent_runner: shell.agent_runner.clone(),
            allow_all: shell.allow_all,
            max_tool_concurrency: shell.max_tool_concurrency,
            max_agent_concurrency: shell.max_agent_concurrency,
            agent_semaphore: shell.agent_semaphore.clone(),
            memory_config: super::mapping::memory_config_to_sdk(shell.memory_config.clone()),
            skill_snapshot: super::mapping::skill_snapshot_to_sdk(
                shell.initial_skill_snapshot.clone(),
            ),
            hook_runner: services.hooks.clone(),
            session_reminders: Arc::new(std::sync::Mutex::new(tools::SessionReminders::new())),
            workspace_root: shell.cwd.clone(),
        }
    }
}
