//! AgentClientImpl / RuntimeHandle 结构体定义与公共访问器。

use std::sync::Arc;

use crate::application::main_loop::ChatEventSinkHandle;
use crate::ports::legacy::ChatRuntimeContext;
use sdk::ChatEvent;
use share::config::models::ResolvedModel;

/// #1381: Composition-injected input port pair.
/// Application receives this without importing adapter modules.
pub struct InputPortPair {
    pub queue: crate::adapters::input_buffer::RuntimeQueueDrainPort,
    pub input_events: crate::adapters::input_buffer::RuntimeInputEventDrainPort,
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
pub struct RuntimeHandle {
    // ─── 业务状态 ───
    pub context: ChatRuntimeContext,
    pub cwd: std::path::PathBuf,
    pub resolved_model: ResolvedModel,
    pub session_id: String,
    pub max_tool_concurrency: usize,
    pub max_agent_concurrency: usize,

    // ─── 可切换的客户端（switch_model 更新此处） ───
    pub(crate) current_binding: std::sync::RwLock<Arc<crate::ports::ProviderBinding>>,

    // ─── SDK 状态 ───
    /// 当前 active Run 的唯一注册表。同步 cancel_run(run_id) 在同一锁内校验 ID、
    /// 标记 Cancelling 并触发该 Run 的 token。
    pub(crate) active_run: Arc<crate::application::active_run::ActiveRunRegistry>,
    /// Interaction bridge：拥有 typed interaction waiter，
    /// 由 `AgentClient::reply_interaction` / `cancel_interaction` 委托。
    pub(crate) interaction_bridge: Arc<crate::application::interaction::InteractionBridge>,
    /// Resume 标志：load_session 后设为 true，chat_impl 消费后重置为 false。
    ///
    /// loop-top idle 门据此在首次遇到 pending user turn 时强制 idle 等待，
    pub(crate) workspace: project::WorkspaceViews,
    /// Context-owned Main Session coordinator — provides admission gate,
    /// session resume, and gate-aware config façades.
    pub(crate) wiring: Arc<context::MainSessionWiring>,
    pub(crate) config_query: Arc<dyn config::ConfigQuery>,
    pub(crate) config_writer: Arc<dyn config::ConfigWriter>,
    /// Same Composition-injected instance borrowed by MainSessionWiring for resume.
    pub(crate) session_management: Arc<dyn context::SessionManagementPort>,
    pub(crate) event_sink_factory: Arc<
        dyn Fn(tokio::sync::mpsc::UnboundedSender<ChatEvent>) -> ChatEventSinkHandle + Send + Sync,
    >,
    /// #1381: Factory for input drain ports, injected by composition.
    /// Application calls this to get concrete QueueDrainPort/InputEventDrainPort
    /// without depending on adapter types directly.
    pub(crate) input_port_factory: Arc<
        dyn Fn(
                Option<Arc<dyn sdk::QueueDrainPort>>,
                Option<Arc<dyn sdk::ChatInputEventPort>>,
            ) -> InputPortPair
            + Send
            + Sync,
    >,

    // ─── SDK 业务对象 ───
    /// Session reminders（供 SDK 增删改查）
    pub(crate) session_reminders:
        std::sync::Arc<std::sync::RwLock<share::memory::SessionReminders>>,
}

// ─── 公共访问器（CLI runtime.rs 需要） ───

impl AgentClientImpl {
    pub fn session_id(&self) -> &str {
        &self.inner.session_id
    }

    pub fn cwd(&self) -> &std::path::Path {
        &self.inner.cwd
    }

    pub fn resolved_model(&self) -> &ResolvedModel {
        &self.inner.resolved_model
    }

    pub fn context(&self) -> &ChatRuntimeContext {
        &self.inner.context
    }

    pub fn max_tool_concurrency(&self) -> usize {
        self.inner.max_tool_concurrency
    }

    pub fn max_agent_concurrency(&self) -> usize {
        self.inner.max_agent_concurrency
    }

    pub fn tui_launch_context(&self) -> crate::adapters::tui_launch::TuiLaunchContext {
        let ctx = self.context().clone();
        crate::adapters::tui_launch::TuiLaunchContext {
            session_id: self.session_id().to_string(),
            model_display: super::mapping::model_display(
                &self.resolved_model().source_key,
                &self.resolved_model().model.name,
                &self.resolved_model().model.id,
            ),
            binding: self.inner.current_binding.read().unwrap().clone(),
            tool_catalog: ctx.resources.tool_catalog,
            tool_execution: ctx.resources.tool_execution,
            system_blocks: ctx.resources.system_blocks,
            system_prompt_text: ctx.resources.system_prompt_text,
            initial_git_context: ctx.resources.initial_git_context,
            user_context: ctx.resources.user_context,
            context_size: ctx.resources.context_size,
            verbose: ctx.verbose,
            agent_runner: ctx.resources.agent_runner,
            allow_all: self.inner.wiring.committed_config().allow_all(),
            max_tool_concurrency: self.max_tool_concurrency(),
            max_agent_concurrency: self.max_agent_concurrency(),
            agent_semaphore: ctx.resources.agent_semaphore,
            memory_config: super::mapping::memory_config_to_sdk(ctx.resources.memory_config),
            skills_map: ctx.resources.skills_map,
            hook_runner: ctx.resources.hook_runner,
            session_reminders: Arc::new(std::sync::Mutex::new(tools::SessionReminders::new())),
            workspace_root: self.inner.cwd.clone(),
        }
    }
}
