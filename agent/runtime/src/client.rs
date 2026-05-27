//! AgentClient 实现 — 封装全部初始化编排。
//!
//! `AgentClientImpl::from_args()` 替代了原 CLI 的 `setup.rs`。
//! 所有 build_* 逻辑在此完成，CLI 只需一行调用。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use sdk::{
    AgentClient, AgentProgressEventView, AgentProgressKindView, AgentToolCallProgressView,
    ChangeSet, ChatEvent, ChatRequest, ChatStream, ClipboardImageView, CostInfo, MemoryConfigView,
    ModelSummary, ProjectContext, ReflectionConfigView, ReflectionMemorySuggestionView,
    ReflectionOutputView, SdkError, SessionSnapshot, SessionSummary, SkillView, TaskStatusView,
    TaskSummary, ToolResultImage, WorkspaceContextView, WorkspaceStackEntryView,
};
use tokio::sync::watch;

use crate::api::core::config::models::ResolvedModel;
use crate::api::core::config::ConfigManager;
use crate::api::core::task::{TaskStatus, TaskStore};
use crate::api::core::tool::ToolRegistry;
use crate::api::prompt::skill::{load_all_skills, Skill};
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
    current_cancel: Arc<Mutex<Option<tokio_util::sync::CancellationToken>>>,
    current_messages: Arc<Mutex<Vec<crate::api::core::message::Message>>>,
    workspace_context: Arc<Mutex<Option<crate::session::WorkspaceContext>>>,
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
        current_cancel: Arc::new(Mutex::new(None)),
        current_messages: Arc::new(Mutex::new(Vec::new())),
        workspace_context: Arc::new(Mutex::new(None)),
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

fn memory_config_to_sdk(config: crate::api::core::config::MemoryConfig) -> MemoryConfigView {
    MemoryConfigView {
        enabled: config.enabled,
        max_entries: config.max_entries,
        similarity_threshold: config.similarity_threshold as f32,
        reflection: ReflectionConfigView {
            enabled: config.reflection.enabled,
            interval_turns: config.reflection.interval_turns,
            auto_apply_suggestions: config.reflection.auto_apply_suggestions,
        },
    }
}

fn skill_to_sdk(skill: Skill) -> SkillView {
    SkillView {
        name: skill.name,
        aliases: skill.aliases,
        description: Some(skill.description),
        content: skill.content,
        source: Some(skill.source_path.display().to_string()),
    }
}

fn processed_image_to_sdk(image: crate::api::image::ProcessedImage) -> ClipboardImageView {
    ClipboardImageView {
        base64: image.base64,
        media_type: image.media_type,
        final_size: image.final_size,
        display_path: None,
        width: None,
        height: None,
    }
}

fn reflection_output_to_sdk(
    output: crate::api::reflection::ReflectionOutput,
    input_tokens: u32,
    output_tokens: u32,
) -> ReflectionOutputView {
    ReflectionOutputView {
        content: crate::api::reflection::ReflectionEngine::format_output(&output),
        input_tokens,
        output_tokens,
        suggested_memories: output
            .suggested_memories
            .into_iter()
            .map(|memory| ReflectionMemorySuggestionView {
                content: memory.content,
                layer: format!("{:?}", memory.category).to_lowercase(),
            })
            .collect(),
        outdated_memories: output.outdated_memories,
    }
}

fn session_summary_from_runtime(session: crate::session::Session) -> SessionSummary {
    let preview = session
        .messages
        .iter()
        .find(|m| m.role == crate::api::core::message::Role::User)
        .map(|m| m.text_content())
        .and_then(|text| {
            let first_line = text.lines().next().unwrap_or("").trim();
            if first_line.is_empty() {
                None
            } else {
                Some(first_line.chars().take(50).collect())
            }
        });
    let summary = session.summary();
    SessionSummary {
        id: session.id,
        title: session.metadata.title,
        project: session.metadata.project,
        model: session.metadata.model,
        created_at: session.created_at,
        updated_at: session.updated_at,
        message_count: session.messages.len(),
        preview,
        summary,
    }
}

fn task_status_lines(
    tasks: &[crate::api::core::task::Task],
    display_map: &std::collections::HashMap<String, usize>,
    max_lines: usize,
) -> Vec<String> {
    if tasks.is_empty() || max_lines == 0 {
        return Vec::new();
    }

    let total = tasks.len();
    let completed_count = tasks
        .iter()
        .filter(|t| t.status == TaskStatus::Completed)
        .count();
    let mut lines = vec![format!("━━ Tasks: {}/{} ━━", completed_count, total)];

    let mut completed: Vec<&crate::api::core::task::Task> = Vec::new();
    let mut in_progress: Vec<&crate::api::core::task::Task> = Vec::new();
    let mut pending: Vec<&crate::api::core::task::Task> = Vec::new();
    for task in tasks {
        match task.status {
            TaskStatus::Completed => completed.push(task),
            TaskStatus::InProgress => in_progress.push(task),
            TaskStatus::Pending => pending.push(task),
            TaskStatus::Deleted => {}
        }
    }
    completed.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    in_progress.sort_by_key(|t| t.updated_at);
    pending.sort_by_key(|t| display_map.get(&t.id).copied().unwrap_or(usize::MAX));

    let ordered: Vec<_> = completed
        .into_iter()
        .chain(in_progress)
        .chain(pending)
        .collect();
    let shown_count = ordered.len().min(max_lines);
    let hidden_count = ordered.len() - shown_count;
    for task in ordered.iter().take(shown_count) {
        lines.push(format_task_status_line(task, display_map));
    }
    if hidden_count > 0 {
        lines.push(format!("… +{} more", hidden_count));
    }
    lines
}

fn format_task_status_line(
    task: &crate::api::core::task::Task,
    display_map: &std::collections::HashMap<String, usize>,
) -> String {
    let icon = match task.status {
        TaskStatus::Completed => "✓",
        TaskStatus::InProgress => "■",
        TaskStatus::Pending => "□",
        TaskStatus::Deleted => "?",
    };
    let display_id = display_map.get(&task.id).copied().unwrap_or(0);
    let owner = task
        .owner
        .as_deref()
        .map(|owner| format!(" (@{})", owner))
        .unwrap_or_default();
    format!("{} #{} {}{}", icon, display_id, task.subject, owner)
}

#[derive(Clone)]
struct SdkChatEventSink {
    tx: tokio::sync::mpsc::UnboundedSender<ChatEvent>,
    current_messages: Arc<Mutex<Vec<crate::api::core::message::Message>>>,
    workspace_context: Arc<Mutex<Option<crate::session::WorkspaceContext>>>,
}

impl crate::chat::ChatEventSink for SdkChatEventSink {
    fn send_event<'a>(
        &'a self,
        event: crate::chat::RuntimeStreamEvent,
    ) -> crate::chat::EventFuture<'a> {
        Box::pin(async move {
            let _ = self.tx.send(runtime_event_to_sdk_event(
                event,
                &self.current_messages,
                &self.workspace_context,
            ));
        })
    }

    fn try_send_event(&self, event: crate::chat::RuntimeStreamEvent) {
        let _ = self.tx.send(runtime_event_to_sdk_event(
            event,
            &self.current_messages,
            &self.workspace_context,
        ));
    }
}

#[derive(Clone, Default)]
struct EmptyQueueDrainPort;

impl crate::chat::QueueDrainPort for EmptyQueueDrainPort {
    fn drain_queued_input<'a>(&'a self) -> crate::chat::QueueFuture<'a> {
        Box::pin(async { None })
    }
}

fn runtime_event_to_sdk_event(
    event: crate::chat::RuntimeStreamEvent,
    current_messages: &Arc<Mutex<Vec<crate::api::core::message::Message>>>,
    workspace_context: &Arc<Mutex<Option<crate::session::WorkspaceContext>>>,
) -> ChatEvent {
    match event {
        crate::chat::RuntimeStreamEvent::Text(text) => ChatEvent::Token(text),
        crate::chat::RuntimeStreamEvent::Thinking(text) => ChatEvent::Thinking(text),
        crate::chat::RuntimeStreamEvent::TextBlockComplete(text) => {
            ChatEvent::TextBlockComplete(text)
        }
        crate::chat::RuntimeStreamEvent::ToolCallStart { name, index } => {
            ChatEvent::ToolCallStart { name, index }
        }
        crate::chat::RuntimeStreamEvent::ToolArgumentsDelta {
            index,
            name,
            partial_args,
        } => ChatEvent::ToolArgumentsDelta {
            index,
            name,
            partial_args,
        },
        crate::chat::RuntimeStreamEvent::ToolCall { id, name, summary } => {
            ChatEvent::ToolCall { id, name, summary }
        }
        crate::chat::RuntimeStreamEvent::ToolResult {
            id,
            tool_name,
            output,
            is_error,
            images,
        } => ChatEvent::ToolResult {
            id,
            tool_name,
            output,
            is_error,
            images: images
                .into_iter()
                .map(|image| ToolResultImage {
                    base64: image.base64,
                    media_type: image.media_type,
                })
                .collect(),
        },
        crate::chat::RuntimeStreamEvent::SystemMessage(msg) => ChatEvent::SystemMessage(msg),
        crate::chat::RuntimeStreamEvent::Error(msg) => ChatEvent::Error(msg),
        crate::chat::RuntimeStreamEvent::Usage {
            input,
            output,
            last_input,
            elapsed_secs,
        } => ChatEvent::Usage {
            input,
            output,
            last_input,
            elapsed_secs,
        },
        crate::chat::RuntimeStreamEvent::MessagesSync(messages) => {
            if let Ok(mut guard) = current_messages.lock() {
                *guard = messages.clone();
            }
            ChatEvent::MessagesSync(messages.into_iter().map(message_to_sdk).collect())
        }
        crate::chat::RuntimeStreamEvent::Done => ChatEvent::Done,
        crate::chat::RuntimeStreamEvent::DoneWithDuration(duration) => {
            ChatEvent::DoneWithDurationMs(duration.as_millis() as u64)
        }
        crate::chat::RuntimeStreamEvent::Cancelled => ChatEvent::Cancelled,
        crate::chat::RuntimeStreamEvent::LiveTps(tps) => ChatEvent::LiveTps(tps),
        crate::chat::RuntimeStreamEvent::TurnChanged(turn) => ChatEvent::CurrentTurnChanged(turn),
        crate::chat::RuntimeStreamEvent::StopFailureHook {
            system_message,
            additional_context,
        } => ChatEvent::StopFailureHook {
            system_message,
            additional_context,
        },
        crate::chat::RuntimeStreamEvent::AskUser {
            id,
            question,
            options,
            allow_free_input,
            multi_select,
            default,
            reply_tx,
        } => ChatEvent::AskUser {
            id,
            question,
            options,
            allow_free_input,
            multi_select,
            default,
            reply_tx,
        },
        crate::chat::RuntimeStreamEvent::AgentProgress { tool_id, event } => {
            ChatEvent::AgentProgress {
                tool_id,
                event: agent_progress_event_to_sdk(event),
            }
        }
        crate::chat::RuntimeStreamEvent::HookStart { event, command } => {
            ChatEvent::HookStart { event, command }
        }
        crate::chat::RuntimeStreamEvent::HookEnd {
            event,
            blocked,
            error,
        } => ChatEvent::HookEnd {
            event,
            blocked,
            error,
        },
        crate::chat::RuntimeStreamEvent::WorkingDirectoryChanged {
            path_base,
            working_root,
            workspace,
        } => {
            if let Ok(mut guard) = workspace_context.lock() {
                *guard = Some(workspace.clone());
            }
            ChatEvent::WorkingDirectoryChanged {
                path_base,
                working_root,
                workspace: workspace_context_to_sdk(workspace),
            }
        }
    }
}

fn agent_progress_event_to_sdk(
    event: crate::api::core::tool::AgentProgressEvent,
) -> AgentProgressEventView {
    let kind = match event.kind {
        crate::api::core::tool::AgentProgressKind::ToolCalls { calls } => {
            AgentProgressKindView::ToolCalls {
                calls: calls
                    .into_iter()
                    .map(|call| AgentToolCallProgressView {
                        id: call.id,
                        name: call.name,
                        input: call.input,
                        summary: call.summary,
                    })
                    .collect(),
            }
        }
        crate::api::core::tool::AgentProgressKind::Message { text } => {
            AgentProgressKindView::Message { text }
        }
    };
    AgentProgressEventView {
        sequence: event.sequence,
        kind,
    }
}

fn workspace_context_to_sdk(workspace: crate::session::WorkspaceContext) -> WorkspaceContextView {
    WorkspaceContextView {
        path_base: workspace.path_base.into(),
        working_root: workspace.working_root.into(),
        context_stack: workspace
            .context_stack
            .into_iter()
            .map(|entry| WorkspaceStackEntryView {
                path_base: entry.path_base.into(),
                working_root: entry.working_root.into(),
            })
            .collect(),
    }
}

fn message_to_sdk(message: crate::api::core::message::Message) -> sdk::ChatMessage {
    sdk::ChatMessage {
        role: match message.role {
            crate::api::core::message::Role::User => "user".to_string(),
            crate::api::core::message::Role::Assistant => "assistant".to_string(),
        },
        content: serde_json::to_value(&message.content).unwrap_or(serde_json::Value::Null),
    }
}

fn message_from_sdk(message: sdk::ChatMessage) -> crate::api::core::message::Message {
    let role = match message.role.as_str() {
        "assistant" => crate::api::core::message::Role::Assistant,
        _ => crate::api::core::message::Role::User,
    };
    let content = serde_json::from_value(message.content).unwrap_or_else(|_| {
        vec![crate::api::core::message::ContentBlock::Text {
            text: String::new(),
        }]
    });
    crate::api::core::message::Message { role, content }
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

    async fn task_status(&self) -> Result<TaskStatusView, SdkError> {
        let tasks = self.inner.context.task_store.list_current_batch().await;
        let active: Vec<_> = tasks
            .iter()
            .filter(|t| t.status != TaskStatus::Deleted)
            .cloned()
            .collect();
        if active.is_empty() {
            return Ok(TaskStatusView::default());
        }

        let display_map = self.inner.context.task_store.get_batch_display_map().await;
        let max_lines = crate::api::core::config::TaskListConfig::default().max_lines;
        let lines = task_status_lines(&active, &display_map, max_lines);
        Ok(TaskStatusView { lines })
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

    async fn chat(&self, input: ChatRequest) -> Result<ChatStream, SdkError> {
        self.inner.cancel_token.store(false, Ordering::Release);
        let cancel = tokio_util::sync::CancellationToken::new();
        *self
            .inner
            .current_cancel
            .lock()
            .map_err(|_| SdkError::Internal("当前 chat 取消锁已损坏".to_string()))? =
            Some(cancel.clone());
        let messages: Vec<_> = input.messages.into_iter().map(message_from_sdk).collect();
        *self
            .inner
            .current_messages
            .lock()
            .map_err(|_| SdkError::Internal("当前 session 消息锁已损坏".to_string()))? =
            messages.clone();

        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let sink = SdkChatEventSink {
            tx,
            current_messages: self.inner.current_messages.clone(),
            workspace_context: self.inner.workspace_context.clone(),
        };
        let inner = self.inner.clone();
        tokio::spawn(async move {
            crate::chat::process_chat_loop(crate::chat::ChatLoopContext {
                sink,
                queue: EmptyQueueDrainPort,
                client: inner.context.client.clone(),
                registry: inner.context.registry.clone(),
                system_blocks: inner.context.system_blocks.clone(),
                system_prompt_text: inner.context.system_prompt_text.clone(),
                user_context: inner.context.user_context.clone(),
                messages,
                context_size: inner.context.context_size,
                cwd: inner.cwd.clone(),
                workspace_context: inner.workspace_context.lock().ok().and_then(|g| g.clone()),
                session_id: inner.session_id.clone(),
                read_files: Arc::new(Mutex::new(std::collections::HashSet::new())),
                session_reminders: Arc::new(Mutex::new(Default::default())),
                agent_runner: Some(inner.context.agent_runner.clone()),
                allow_all: inner.context.allow_all,
                interrupted: inner.cancel_token.clone(),
                cancel,
                task_store: inner.context.task_store.clone(),
                max_tool_concurrency: inner.max_tool_concurrency,
                max_agent_concurrency: inner.max_agent_concurrency,
                agent_semaphore: inner.context.agent_semaphore.clone(),
                hook_runner: inner.context.hook_runner.clone(),
                memory_config: inner.context.memory_config.clone(),
                json_logger: inner.context.json_logger.clone(),
            })
            .await;
            if let Ok(mut guard) = inner.current_cancel.lock() {
                *guard = None;
            }
        });
        Ok(ChatStream::new(rx))
    }

    async fn sync_current_messages(&self, messages: Vec<sdk::ChatMessage>) -> Result<(), SdkError> {
        *self
            .inner
            .current_messages
            .lock()
            .map_err(|_| SdkError::Internal("当前 session 消息锁已损坏".to_string()))? =
            messages.into_iter().map(message_from_sdk).collect();
        Ok(())
    }

    async fn save_current_session(&self) -> Result<(), SdkError> {
        let messages = self
            .inner
            .current_messages
            .lock()
            .map_err(|_| SdkError::Internal("当前 session 消息锁已损坏".to_string()))?
            .clone();
        let task_snapshot = {
            let snap = self.inner.context.task_store.snapshot().await;
            if snap.tasks.is_empty() {
                None
            } else {
                Some(snap)
            }
        };
        let workspace = self
            .inner
            .workspace_context
            .lock()
            .map_err(|_| SdkError::Internal("当前工作区上下文锁已损坏".to_string()))?
            .clone();
        let mut session = crate::session::Session::new(
            self.inner.session_id.clone(),
            self.inner.cwd.to_string_lossy().to_string(),
        );
        session.messages = messages;
        session.updated_at = crate::session::now_iso();
        session.metadata.model = Some(model_display(
            &self.inner.resolved_model.source_key,
            &self.inner.resolved_model.model.name,
            &self.inner.resolved_model.model.id,
        ));
        session.tasks = task_snapshot;
        session.workspace = workspace;
        crate::session::save_session(&session)
            .await
            .map_err(SdkError::Session)
    }

    fn cancel(&self) {
        self.inner.cancel_token.store(true, Ordering::Release);
        if let Ok(guard) = self.inner.current_cancel.lock() {
            if let Some(token) = guard.as_ref() {
                token.cancel();
            }
        }
    }

    async fn load_session(&self, _id: &str) -> Result<SessionSnapshot, SdkError> {
        Ok(SessionSnapshot {
            id: _id.to_string(),
            message_count: 0,
            total_tokens: 0,
        })
    }

    async fn list_sessions(&self) -> Result<Vec<SessionSummary>, SdkError> {
        Ok(crate::session::list_sessions()
            .await
            .into_iter()
            .map(session_summary_from_runtime)
            .collect())
    }

    async fn delete_session(&self, id: &str) -> Result<(), SdkError> {
        crate::session::delete_session(id)
            .await
            .map_err(SdkError::Session)
    }

    async fn list_models(&self) -> Result<Vec<ModelSummary>, SdkError> {
        let config = ConfigManager::new(Some(&self.inner.cwd))
            .load()
            .await
            .map_err(SdkError::Init)?;
        Ok(config
            .models
            .list_models()
            .into_iter()
            .map(|(provider, model)| ModelSummary {
                provider,
                id: model.id,
                name: model.name,
                context_window: model.context_window,
                max_tokens: model.max_tokens,
            })
            .collect())
    }

    async fn compact(&self) -> Result<(), SdkError> {
        Ok(())
    }

    async fn read_clipboard_image(&self) -> Result<ClipboardImageView, SdkError> {
        crate::api::image::read_clipboard_image()
            .await
            .map(processed_image_to_sdk)
            .map_err(|e| SdkError::Internal(e.to_string()))
    }

    async fn process_image_file(&self, path: String) -> Result<ClipboardImageView, SdkError> {
        crate::api::image::process_image_file(&path)
            .await
            .map(processed_image_to_sdk)
            .map_err(|e| SdkError::Internal(e.to_string()))
    }

    async fn run_reflection(
        &self,
        messages: Vec<sdk::ChatMessage>,
    ) -> Result<ReflectionOutputView, SdkError> {
        let runtime_messages = messages
            .into_iter()
            .map(message_from_sdk)
            .collect::<Vec<_>>();
        let recent_summary = crate::api::reflection::ReflectionEngine::recent_messages_summary(
            &runtime_messages,
            6000,
        );
        let output = crate::api::reflection::ReflectionOutput {
            deviations: vec![recent_summary],
            suggested_memories: Vec::new(),
            outdated_memories: Vec::new(),
            user_alert: None,
        };
        Ok(reflection_output_to_sdk(output, 0, 0))
    }

    async fn apply_reflection(&self, output: ReflectionOutputView) -> Result<String, SdkError> {
        let count = output.suggested_memories.len();
        Ok(format!(
            "已生成 {count} 条记忆建议；自动写入将在后续 SDK memory 能力中接入"
        ))
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

    pub fn tui_launch_context(&self) -> crate::tui_launch::TuiLaunchContext {
        let ctx = self.context().clone();
        crate::tui_launch::TuiLaunchContext {
            session_id: self.session_id().to_string(),
            cwd: self.cwd().to_path_buf(),
            model_display: model_display(
                &self.resolved_model().source_key,
                &self.resolved_model().model.name,
                &self.resolved_model().model.id,
            ),
            client: ctx.client,
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
            memory_config: memory_config_to_sdk(ctx.memory_config),
            skills_map: ctx
                .skills_map
                .into_iter()
                .map(|(name, skill)| (name, skill_to_sdk(skill)))
                .collect(),
            hook_runner: ctx.hook_runner,
            json_logger: ctx.json_logger,
        }
    }
}

fn model_display(source_key: &str, model_name: &str, model_id: &str) -> String {
    let display_name = if model_name.is_empty() {
        model_id
    } else {
        model_name
    };
    format!("{}/{}", source_key, display_name)
}
