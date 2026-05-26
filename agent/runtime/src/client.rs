//! AgentClient 实现 — 封装全部初始化编排。
//!
//! `AgentClientImpl::from_args()` 替代了原 CLI 的 `setup.rs`。
//! 所有 build_* 逻辑在此完成，CLI 只需一行调用。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use sdk::{
    AgentClient, ChangeSet, ChatInput, ChatStream, CostInfo, ProjectContext, SdkError,
    SessionSnapshot, TaskSummary,
};
use tokio::sync::watch;

use crate::api::core::config::models::ResolvedModel;
use crate::api::core::config::ConfigManager;
use crate::api::core::skill_ops::{load_all_skills, Skill};
use crate::api::core::task::TaskStore;
use crate::api::core::tool::ToolRegistry;
use crate::api::prompt_build::{build_system_prompt_parts, PromptContext};
use crate::api::provider::types::SystemBlock;
use crate::api::tools as tools_crate;
use crate::api::tools::mcp_manager::McpConnectionManager;
use crate::bootstrap::{
    self, apply_config_permission_mode, build_agent_runner, build_hook_runner, build_json_logger,
    init_logging, resolve_api_key, resolve_base_url, resolve_concurrency_limits,
    resolve_context_size, resolve_model_runtime_settings, spawn_mcp_connect, ReasoningConfigInput,
};
use crate::bootstrap::{set_session_id, start_session, ChatBootstrapArgs};
use crate::chat::ChatRuntimeContext;

/// AgentClient 的 runtime 实现。
///
/// 持有全部运行时状态（LLM client、tool registry、session 等），
/// CLI 通过 sdk::AgentClient trait 与之交互。
#[derive(Clone)]
pub struct AgentClientImpl {
    inner: Arc<RuntimeHandle>,
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

    // ─── SDK 状态 ───
    cancel_token: Arc<AtomicBool>,
    change_tx: watch::Sender<ChangeSet>,
    change_rx: watch::Receiver<ChangeSet>,
}

impl RuntimeHandle {
    pub fn notify_change(&self, set: ChangeSet) {
        let _ = self.change_tx.send(set);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel_token.load(Ordering::Acquire)
    }
}

/// 从 Args 初始化 AgentClient。
///
/// 模型选择直接使用 `Config.models.select_for_run()`，无需外部注入。
pub async fn from_args(mut args: ChatBootstrapArgs) -> Result<AgentClientImpl, SdkError> {
    // 1. Guidance 目录初始化
    crate::api::prompt::guidance::init_guidance_dir();

    // 2. 解析 cwd
    let cwd = args
        .cwd
        .clone()
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| std::path::PathBuf::from("."));

    // 3. 加载配置
    let config_file = ConfigManager::new(Some(&cwd)).load().await.ok();

    // 4. 日志初始化
    init_logging(
        config_file
            .as_ref()
            .map(|c| &c.logging)
            .unwrap_or(&crate::api::core::config::LoggingConfig::default()),
    );

    // 5. 权限模式
    apply_config_permission_mode(&mut args, config_file.as_ref());

    // 6. 模型选择 — 直接使用 ModelsConfig::select_for_run
    let config = config_file.as_ref().ok_or_else(|| {
        SdkError::Init(
            "未指定模型。请使用 --model <来源>/<模型>，或在 ~/.agents/aemeath.json 配置 models.default".to_string(),
        )
    })?;
    let resolved_model = config
        .models
        .select_for_run(args.model.as_deref())
        .map_err(|e| SdkError::Init(e.to_string()))?;
    let api_type = resolved_model.api;

    // 7. API key
    let api_key = resolve_api_key(args.api_key.take(), &resolved_model, None).ok_or_else(|| {
        SdkError::Init(
            "API key not set. Use --api-key, set provider-specific env var, set LLM_API_KEY, or configure in ~/.aemeath/config.json".to_string(),
        )
    })?;

    // 8. Base URL + model + runtime settings
    let base_url = resolve_base_url(args.base_url.clone(), &resolved_model);
    let model = resolved_model.model.id.clone();
    let runtime_settings = resolve_model_runtime_settings(
        args.max_tokens,
        &resolved_model.model,
        config_file.as_ref(),
        !args.no_think,
        ReasoningConfigInput {
            cli_reasoning_effort: args.reasoning_effort.clone(),
            env_reasoning_effort: std::env::var("AEMEATH_REASONING_EFFORT").ok(),
        },
    )
    .map_err(|e| SdkError::Init(e.to_string()))?;

    log::info!(
        "[main] source={} api={} model={} reasoning={} effort={:?} args.no_think={}",
        resolved_model.source_key,
        api_type.as_str(),
        model,
        runtime_settings.reasoning,
        runtime_settings.reasoning_effort,
        args.no_think
    );

    // 9. LLM client
    let client = Arc::new(bootstrap::build_llm_client(
        api_type,
        api_key,
        base_url,
        model.clone(),
        &resolved_model,
        &runtime_settings,
    ));

    // 10. Tooling
    let task_store = Arc::new(TaskStore::new());
    let skills_map = load_configured_skills(&cwd, config_file.as_ref().map(|c| &c.skills));
    if !skills_map.is_empty() {
        log::info!("[Skills] loaded {} skills", skills_map.len());
    }
    let skills = Arc::new(tokio::sync::Mutex::new(skills_map.clone()));
    let registry = {
        let reg = ToolRegistry::new();
        tools_crate::register_all_tools(&reg, task_store.clone(), skills.clone());
        Arc::new(reg)
    };
    let mcp_manager = spawn_mcp_connect(registry.clone(), &cwd).await;

    // 11. Hook runner
    let hook_runner = build_hook_runner(config_file.as_ref(), &cwd);

    // 12. Session
    let session_id = start_session(args.resume.clone());
    set_session_id(session_id.clone());

    // 13. JSON logger
    let json_logger = build_json_logger(&session_id, config_file.as_ref());

    // 14. Agent runner
    let agent_runner = build_agent_runner(
        config_file.as_ref(),
        client.clone(),
        hook_runner.clone(),
        runtime_settings.reasoning,
        json_logger.clone(),
    );

    // 15. Prompt bundle
    let prompt_memory_config = config_file
        .as_ref()
        .map(|c| c.memory.clone())
        .unwrap_or_default();
    let prompt_context = PromptContext::new(
        &cwd,
        Some(client.provider_name()),
        Some(client.model_name()),
    );
    let prompt_parts =
        build_system_prompt_parts(&prompt_context, &hook_runner, &prompt_memory_config).await;

    let static_prompt = crate::prompt_build_ext::build_static_prompt(
        &cwd,
        &model,
        runtime_settings.reasoning,
        config_file.as_ref(),
        &hook_runner,
        prompt_parts.clone(),
        &skills,
    )
    .await;
    let system_blocks = vec![
        SystemBlock::cached(static_prompt),
        SystemBlock::dynamic(prompt_parts.dynamic_part),
    ];
    let system_prompt_text: String = system_blocks
        .iter()
        .map(|b| b.text.as_str())
        .collect::<Vec<_>>()
        .join("\n\n");

    // 16. Concurrency
    let (max_tool_concurrency, max_agent_concurrency) = resolve_concurrency_limits(
        args.max_tool_concurrency,
        args.max_agent_concurrency,
        config_file.as_ref(),
    );
    let agent_semaphore = Arc::new(tokio::sync::Semaphore::new(max_agent_concurrency));
    log::info!(
        "concurrency limits: max_tool={}, max_agent={}",
        max_tool_concurrency,
        max_agent_concurrency
    );

    // 17. context_size / verbose 合并
    let context_size = resolve_context_size(args.context_size, config_file.as_ref());

    // 18. 组装 context
    let memory_config = config_file
        .as_ref()
        .map(|c| c.memory.clone())
        .unwrap_or_default();
    let context = ChatRuntimeContext {
        client,
        registry,
        system_blocks,
        system_prompt_text,
        user_context: prompt_parts.claude_md,
        agent_runner,
        task_store,
        skills_map,
        hook_runner,
        memory_config,
        json_logger,
        agent_semaphore,
        allow_all: args.allow_all,
        context_size,
        verbose: args.verbose,
        resume: args.resume,
    };

    // 19. 构建 handle
    let (change_tx, change_rx) = watch::channel(ChangeSet::empty());
    let handle = RuntimeHandle {
        context,
        cwd,
        resolved_model,
        session_id,
        max_tool_concurrency,
        max_agent_concurrency,
        _mcp_manager: mcp_manager,
        cancel_token: Arc::new(AtomicBool::new(false)),
        change_tx,
        change_rx,
    };

    Ok(AgentClientImpl {
        inner: Arc::new(handle),
    })
}

// ─── 内部辅助 ───

fn load_configured_skills(
    cwd: &std::path::Path,
    skills_config: Option<&crate::api::core::config::SkillsConfig>,
) -> std::collections::HashMap<String, Skill> {
    let dirs = skills_config.map(|c| c.dirs.clone()).unwrap_or_default();
    load_all_skills(cwd, &dirs)
}

// ─── AgentClient trait 实现 ───

#[async_trait]
impl AgentClient for AgentClientImpl {
    fn session_snapshot(&self) -> SessionSnapshot {
        SessionSnapshot {
            id: self.inner.session_id.clone(),
            message_count: 0, // TODO: 从实际 session 获取
            total_tokens: 0,
        }
    }

    fn cost(&self) -> CostInfo {
        // TODO: 从 cost_tracker 获取
        CostInfo::default()
    }

    fn task_list(&self) -> Vec<TaskSummary> {
        // TODO: 从 task_store 获取
        Vec::new()
    }

    fn project(&self) -> ProjectContext {
        ProjectContext {
            cwd: self.inner.cwd.to_string_lossy().to_string(),
            path_base: String::new(),    // TODO
            working_root: String::new(), // TODO
            git_branch: None,
        }
    }

    fn changes(&self) -> watch::Receiver<ChangeSet> {
        self.inner.change_rx.clone()
    }

    async fn chat(&self, _input: ChatInput) -> Result<ChatStream, SdkError> {
        // TODO: 连接到实际 chat loop
        let (_, rx) = tokio::sync::mpsc::unbounded_channel();
        Ok(ChatStream::new(rx))
    }

    fn cancel(&self) {
        self.inner.cancel_token.store(true, Ordering::Release);
    }

    async fn save_session(&self) -> Result<(), SdkError> {
        Ok(())
    }

    async fn load_session(&self, _id: &str) -> Result<SessionSnapshot, SdkError> {
        Ok(SessionSnapshot {
            id: _id.to_string(),
            message_count: 0,
            total_tokens: 0,
        })
    }

    async fn list_sessions(&self) -> Result<Vec<sdk::session::SessionSummary>, SdkError> {
        Ok(Vec::new())
    }

    async fn delete_session(&self, _id: &str) -> Result<(), SdkError> {
        Ok(())
    }

    async fn compact(&self) -> Result<(), SdkError> {
        Ok(())
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
}
