//! AgentClientImpl / RuntimeHandle 结构体定义与公共访问器。

use std::sync::{Arc, Mutex};

use sdk::ChangeSet;
use tokio::sync::watch;

use crate::core::port::ChatRuntimeContext;
use share::config::models::ResolvedModel;
use storage::api::TaskStore;
use tools::api::McpConnectionManager;

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
    pub _mcp_manager: Arc<McpConnectionManager>,

    // ─── 可切换的客户端（switch_model 更新此处） ───
    pub(crate) current_client: std::sync::RwLock<Arc<provider::api::LlmClient>>,

    // ─── SDK 状态 ───
    pub(crate) current_cancel: Arc<Mutex<Option<tokio_util::sync::CancellationToken>>>,
    pub(crate) current_messages: Arc<Mutex<Vec<share::message::Message>>>,
    pub(crate) workspace: Arc<project::api::WorkspaceService>,
    pub(crate) change_tx: watch::Sender<ChangeSet>,
    pub(crate) change_rx: watch::Receiver<ChangeSet>,

    // ─── SDK 业务对象 ───
    /// HookRunner（clone，供 SDK 通知 hook）
    pub(crate) hook_runner: Option<hook::api::HookRunner>,
    /// TaskStore（Arc，供 SDK 恢复任务）
    pub(crate) task_store: Option<std::sync::Arc<TaskStore>>,
    /// Session reminders（供 SDK 增删改查）
    pub(crate) session_reminders:
        std::sync::Arc<std::sync::RwLock<share::memory::SessionReminders>>,
}

impl AgentClientImpl {
    pub fn notify_change(&self, set: ChangeSet) {
        let previous = *self.inner.change_tx.borrow();
        let _ = self.inner.change_tx.send(previous | set);
    }
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

    pub fn tui_launch_context(&self) -> crate::core::tui_launch::TuiLaunchContext {
        let ctx = self.context().clone();
        crate::core::tui_launch::TuiLaunchContext {
            session_id: self.session_id().to_string(),
            cwd: self.cwd().to_path_buf(),
            model_display: super::mapping::model_display(
                &self.resolved_model().source_key,
                &self.resolved_model().model.name,
                &self.resolved_model().model.id,
            ),
            client: self.inner.current_client.read().unwrap().clone(),
            registry: ctx.registry,
            system_blocks: ctx.system_blocks,
            system_prompt_text: ctx.system_prompt_text,
            user_context: ctx.user_context,
            context_size: ctx.context_size,
            verbose: ctx.verbose,
            agent_runner: ctx.agent_runner,
            allow_all: ctx.allow_all,
            task_store: ctx.task_store,
            max_tool_concurrency: self.max_tool_concurrency(),
            max_agent_concurrency: self.max_agent_concurrency(),
            agent_semaphore: ctx.agent_semaphore,
            memory_config: super::mapping::memory_config_to_sdk(ctx.memory_config),
            skills_map: ctx
                .skills_map
                .into_iter()
                .map(|(name, skill)| (name, super::mapping::skill_to_sdk(skill)))
                .collect(),
            hook_runner: ctx.hook_runner,
            session_reminders: Arc::new(
                std::sync::Mutex::new(share::tool::SessionReminders::new()),
            ),
        }
    }
}
