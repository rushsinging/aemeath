//! AgentClientImpl / RuntimeHandle 结构体定义与公共访问器。

use std::collections::HashMap;
use std::sync::Arc;

use crate::application::main_loop::ChatEventSinkHandle;
use crate::application::run_config::RunConfigSnapshot;
use crate::application::runtime_context::{
    ParentRunContextSource, RunCancellationScope, RunInputBufferHandle, RunUsageTracker,
    RuntimeContext, RuntimeContextParts,
};
use crate::domain::agent_run::RunSpec;
use hook::HookPort;
use memory::api::{MemoryPort, ReflectionHistoryStore};
use sdk::ChatEvent;
use share::config::models::ResolvedModel;
use share::config::MemoryConfig;
use task::TaskAccess;
use tools::{AgentRunner, ToolCatalogPort, ToolExecutionContextBindingPort, ToolExecutionPort};
use workflow::api::ReasoningPort;

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
/// via [`MainSessionShell::assemble_main_runtime_context`].
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
    pub(crate) tool_catalog: Arc<dyn ToolCatalogPort>,
    pub(crate) tool_execution: Arc<dyn ToolExecutionPort>,
    pub(crate) tool_context_binding: Arc<dyn ToolExecutionContextBindingPort>,
    pub(crate) hook_runner: Arc<dyn HookPort>,
    pub(crate) policy: Arc<dyn crate::ports::PolicyPort>,
    pub(crate) task_access: Arc<dyn TaskAccess>,
    pub(crate) reflection_history: Arc<dyn ReflectionHistoryStore>,
}

/// Error returned when RuntimeContext assembly fails.
#[derive(Debug, thiserror::Error)]
pub enum RuntimeContextAssemblyError {
    #[error("RunSpec {spec:?} is not supported for Main assembly")]
    UnsupportedSpec { spec: RunSpec },
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
    /// #1385 Task 6: derivation guard failed.
    #[error("sub derivation failed: {reason}")]
    SubDerivationFailed { reason: String },
}

impl MainSessionShell {
    /// Assemble a per-Run [`RuntimeContext`] from this session shell.
    ///
    /// #1385: This is the seam that Task 5 will call when wiring MainRunPort.
    /// Each call produces a fresh cancellation scope and freezes the current
    /// provider binding. Parent capability ports (policy, hook, task, tools,
    /// reflection) are shared via Arc clone.
    ///
    /// `context` and `memory` come from [`context::BoundMainRun`]
    /// (obtained via `wiring.bind_main_run()`). They are bound to the committed
    /// session snapshot and must not outlive the run.
    ///
    /// `config` is the frozen [`RunConfigSnapshot`] — the caller must capture
    /// this from [`context::MainSessionWiring::committed_config()`] or from
    /// the [`context::BoundMainRun`] snapshot.  No default Config / fixed
    /// revision is used inside this method.
    ///
    /// #1385: Per-run adapters for input/events/usage are injected via
    /// RuntimeContext. Input goes through `RunInputBufferHandle`; events through
    /// the real `ChatEventSinkHandle`; usage through `RunUsageTracker`.
    ///
    /// `event_sink` is constructed by the loop_runner from the current session
    /// sink and passed in — no inline noop placeholder.
    ///
    /// `reasoning` is a per-Run adapter; production implementations are wired
    /// in Task 5.
    pub fn assemble_main_runtime_context(
        &self,
        spec: &RunSpec,
        config: RunConfigSnapshot,
        context: Arc<dyn crate::ports::ContextPort>,
        memory: Arc<dyn MemoryPort>,
        reasoning: Arc<dyn ReasoningPort>,
        event_sink: crate::application::main_loop::ChatEventSinkHandle,
    ) -> Result<RuntimeContext, RuntimeContextAssemblyError> {
        if spec.kind != crate::domain::agent_run::RunKind::Main {
            return Err(RuntimeContextAssemblyError::UnsupportedSpec { spec: spec.clone() });
        }

        let binding = self.current_binding.read().unwrap().clone();
        let cancel = RunCancellationScope::new();

        let parts = RuntimeContextParts {
            context,
            provider: binding,
            tool_catalog: self.tool_catalog.clone(),
            tool_execution: self.tool_execution.clone(),
            tool_context_binding: self.tool_context_binding.clone(),
            policy: self.policy.clone(),
            interaction: self.interaction_bridge.clone(),
            memory,
            reflection_history: self.reflection_history.clone(),
            task: self.task_access.clone(),
            hooks: self.hook_runner.clone(),
            reasoning,
            config,
            cancel,
            // #1385 Task 12: I/O seams populated with real per-Run handles from caller.
            event_sink,
            usage: RunUsageTracker::new(),
            input: RunInputBufferHandle::new(),
        };

        Ok(RuntimeContext::new(parts))
    }
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
        crate::adapters::tui_launch::TuiLaunchContext {
            session_id: shell.session_id.clone(),
            model_display: super::mapping::model_display(
                &shell.resolved_model.source_key,
                &shell.resolved_model.model.name,
                &shell.resolved_model.model.id,
            ),
            binding: shell.current_binding.read().unwrap().clone(),
            tool_catalog: shell.tool_catalog.clone(),
            tool_execution: shell.tool_execution.clone(),
            system_blocks: shell.system_blocks.clone(),
            system_prompt_text: shell.system_prompt_text.clone(),
            initial_git_context: shell.initial_git_context.clone(),
            user_context: shell.user_context.clone(),
            context_size: shell.context_size,
            verbose: shell.verbose,
            agent_runner: shell.agent_runner.clone(),
            allow_all: shell.allow_all,
            max_tool_concurrency: shell.max_tool_concurrency,
            max_agent_concurrency: shell.max_agent_concurrency,
            agent_semaphore: shell.agent_semaphore.clone(),
            memory_config: super::mapping::memory_config_to_sdk(shell.memory_config.clone()),
            skills_map: shell.skills_map.clone(),
            hook_runner: shell.hook_runner.clone(),
            session_reminders: Arc::new(std::sync::Mutex::new(tools::SessionReminders::new())),
            workspace_root: shell.cwd.clone(),
        }
    }
}
