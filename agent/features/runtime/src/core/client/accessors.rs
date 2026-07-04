//! AgentClientImpl / RuntimeHandle 结构体定义与公共访问器。

use std::sync::{Arc, Mutex};

use sdk::{ChangeSet, ConfigView};
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
    /// 会话级取消令牌槽（常驻 actor 可重建）。
    ///
    /// 不再用 `Option`：槽内始终持有一个有效 token。chat loop 每回合从此槽读取
    /// 「当前 token」，并在处理完一次取消后把槽**重置为新 token**（见 `loop_runner`），
    /// 以免常驻 loop 中被取消的 token 永久污染后续回合。`cancel_impl` 锁此槽对当前
    /// token 调 `cancel()` 触发取消。`std::sync::Mutex` —— NEVER 跨 `.await` 持有。
    pub(crate) current_cancel: Arc<Mutex<tokio_util::sync::CancellationToken>>,
    pub(crate) current_messages: Arc<Mutex<Vec<share::message::Message>>>,
    /// Compact 时冻结的旧链（保留在 session 文件中供审计，resume 不加载）。
    pub(crate) frozen_chats: Arc<Mutex<Vec<crate::business::session::ChatSegment>>>,
    /// 活跃链的 compact summary（走 system 通道注入）。
    pub(crate) active_summary: Arc<Mutex<Option<String>>>,
    /// Resume 标志：load_session 后设为 true，chat_impl 消费后重置为 false。
    ///
    /// loop-top idle 门据此在首次遇到 pending user turn 时强制 idle 等待，
    /// 而非自动恢复被中断的对话（#503）。
    pub(crate) skip_first_pending_turn: Arc<std::sync::atomic::AtomicBool>,
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
        }
    }
}

pub async fn config_view_impl(this: &super::AgentClientImpl) -> Result<ConfigView, sdk::SdkError> {
    let resolved = this.resolved_model();
    let ctx = this.context();
    let model_display = super::mapping::model_display(
        &resolved.source_key,
        &resolved.model.name,
        &resolved.model.id,
    );
    let api_key = resolved.source_config.api_key.as_str();
    Ok(ConfigView {
        model_name: model_display,
        provider: Some(resolved.source_key.clone()),
        has_api_key: !api_key.is_empty(),
        api_key_preview: if api_key.len() >= 8 {
            Some(api_key[..8].to_string())
        } else if !api_key.is_empty() {
            Some(api_key.to_string())
        } else {
            None
        },
        permission_mode: if ctx.resources.allow_all {
            "allow_all".into()
        } else {
            "ask".into()
        },
        markdown: true,
        verbose: ctx.verbose,
        context_size: ctx.resources.context_size,
        logging_level: String::new(),
    })
}
