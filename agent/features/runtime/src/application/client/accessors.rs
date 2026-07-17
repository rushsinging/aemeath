//! AgentClientImpl / RuntimeHandle 结构体定义与公共访问器。

use std::sync::{Arc, Mutex};

use crate::application::chat::ChatEventSinkHandle;
use crate::ports::legacy::ChatRuntimeContext;
use sdk::ChatEvent;
use share::config::models::ResolvedModel;
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
    pub(crate) current_client: std::sync::RwLock<Arc<provider::LlmClient>>,

    // ─── SDK 状态 ───
    /// 当前 active Run 的唯一注册表。同步 cancel_run(run_id) 在同一锁内校验 ID、
    /// 标记 Cancelling 并触发该 Run 的 token。
    pub(crate) active_run: Arc<crate::application::active_run::ActiveRunRegistry>,
    /// 会话历史唯一活跃真相——按 user turn 分段的 `ChatChain` 聚合。
    ///
    /// 持久化 / 给 LLM / TUI 均为派生投影（`messages_flat()` / `active_segments()`）。
    pub(crate) current_chain: Arc<Mutex<context::session::ChatChain>>,
    /// Compact 时冻结的旧链（保留在 session 文件中供审计，resume 不加载）。
    pub(crate) frozen_chats: Arc<Mutex<Vec<context::session::ChatSegment>>>,
    /// 活跃链的 compact summary（走 system 通道注入）。
    pub(crate) active_summary: Arc<Mutex<Option<String>>>,
    /// Resume 标志：load_session 后设为 true，chat_impl 消费后重置为 false。
    ///
    /// loop-top idle 门据此在首次遇到 pending user turn 时强制 idle 等待，
    pub(crate) workspace: project::WorkspaceViews,
    pub(crate) event_sink_factory: Arc<
        dyn Fn(tokio::sync::mpsc::UnboundedSender<ChatEvent>) -> ChatEventSinkHandle + Send + Sync,
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
            client: self.inner.current_client.read().unwrap().clone(),
            registry: ctx.resources.registry,
            system_blocks: ctx.resources.system_blocks,
            system_prompt_text: ctx.resources.system_prompt_text,
            user_context: ctx.resources.user_context,
            context_size: ctx.resources.context_size,
            verbose: ctx.verbose,
            agent_runner: ctx.resources.agent_runner,
            allow_all: ctx.resources.allow_all,
            task_store: ctx.resources.task_store,
            max_tool_concurrency: self.max_tool_concurrency(),
            max_agent_concurrency: self.max_agent_concurrency(),
            agent_semaphore: ctx.resources.agent_semaphore,
            memory_config: super::mapping::memory_config_to_sdk(ctx.resources.memory_config),
            skills_map: ctx
                .resources
                .skills_map
                .into_iter()
                .map(|(name, skill)| (name, super::mapping::skill_to_sdk(skill)))
                .collect(),
            hook_runner: ctx.resources.hook_runner,
            session_reminders: Arc::new(
                std::sync::Mutex::new(share::tool::SessionReminders::new()),
            ),
            workspace_root: self.inner.cwd.clone(),
        }
    }
}
