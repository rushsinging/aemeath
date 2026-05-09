mod agent_runner;
mod cli;
mod image;
mod mcp_loader;
mod prompt;
mod reflection;
mod render;
mod repl;
mod task_reminder;
mod tui;

use aemeath_core::config::{models::ResolvedModel, Config, ModelEntryConfig};
use aemeath_core::logging::{self, LogFile};
use aemeath_core::provider::ApiDriverKind;
use aemeath_core::tool::ToolRegistry;
use aemeath_llm::client::{LlmClient, OpenAIProviderConfig};
use aemeath_llm::providers::openai_compatible::ReasoningConfig;
use clap::Parser;
use std::env;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::OnceLock;

/// 全局 session ID，供日志格式化器使用
static SESSION_ID: OnceLock<String> = OnceLock::new();
static CURRENT_TURN: AtomicUsize = AtomicUsize::new(0);

/// 设置全局 session ID（只能调用一次）
fn set_session_id(id: String) {
    let _ = SESSION_ID.set(id);
}

pub(crate) fn set_current_turn(turn: usize) {
    CURRENT_TURN.store(turn, Ordering::Relaxed);
}

fn current_turn_for_log() -> Option<usize> {
    match CURRENT_TURN.load(Ordering::Relaxed) {
        0 => None,
        turn => Some(turn),
    }
}

use cli::{Args, Cli, Commands};
use mcp_loader::load_mcp_tools;
use prompt::build_system_prompt_parts;

#[tokio::main]
async fn main() {
    init_panic_hook();

    let cli = Cli::parse();

    match cli.command {
        Some(Commands::Models { json }) => {
            run_models_command(json);
            return;
        }
        Some(Commands::Sessions {
            delete,
            json,
            limit,
        }) => {
            run_sessions_command(delete, json, limit).await;
            return;
        }
        Some(Commands::Run { run_args }) => {
            run_chat(run_args.into()).await;
        }
        None => {
            // 无子命令 — 默认调用 run，使用顶层参数
            run_chat(cli.run_args.into()).await;
        }
    }
}

fn init_logging(logging_config: &aemeath_core::config::LoggingConfig) {
    // 初始化结构化日志 — 路由到 ~/.aemeath/aemeath.log，避免库的 log::warn! / log::error! 破坏 TUI 渲染。
    // 设置 AEMEATH_LOG_STDERR=1 可在使用 --no-tui / CLI 模式调试时恢复 stderr 行为。
    // 日志级别由 config.json 的 logging 段控制；可通过 RUST_LOG 环境变量覆盖。
    let default_filter = logging_config.to_filter_string();
    let mut builder = env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or(&default_filter),
    );
    let use_stderr = std::env::var("AEMEATH_LOG_STDERR")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    if !use_stderr {
        if let Ok(file) = logging::open_append(LogFile::Aemeath) {
            builder.target(env_logger::Target::Pipe(Box::new(file)));
        }
    }
    builder.format(|buf, record| {
        use std::io::Write;
        let session = SESSION_ID.get().map(|s| s.as_str()).unwrap_or("????????");
        writeln!(
            buf,
            "{}",
            logging::format_text_line_with_turn(
                session,
                current_turn_for_log(),
                record.level().as_str(),
                record.module_path().unwrap_or(record.target()),
                &record.args().to_string(),
            )
        )
    });
    builder.init();
}

fn init_panic_hook() {
    std::panic::set_hook(Box::new(move |info| {
        let payload = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "unknown panic".to_string());

        let location = info
            .location()
            .map(|loc| format!("{}:{}:{}", loc.file(), loc.line(), loc.column()))
            .unwrap_or_else(|| "unknown location".to_string());
        let session = SESSION_ID.get().map(|s| s.as_str()).unwrap_or("????????");
        let backtrace = format!("{:?}", std::backtrace::Backtrace::capture());
        let msg = format!("{} at {}", payload, location);
        let extra = serde_json::json!({
            "location": location,
            "backtrace": backtrace,
        });

        let _ = logging::append_json_line_with_turn(
            LogFile::Panic,
            session,
            current_turn_for_log(),
            "ERROR",
            "panic",
            &msg,
            extra,
        );
        eprintln!("[PANIC] {}", msg);
    }));
}

/// 处理 `aemeath models` 子命令
fn format_token_limit_k(tokens: u32) -> String {
    if tokens > 0 {
        format!("{}k", tokens / 1000)
    } else {
        "-".to_string()
    }
}

fn model_row_display(
    provider: &str,
    model: &ModelEntryConfig,
) -> (String, String, String, String, String) {
    let name = if model.name.is_empty() {
        "-".to_string()
    } else {
        model.name.clone()
    };
    (
        provider.to_string(),
        model.id.clone(),
        name,
        format_token_limit_k(model.context_window as u32),
        format_token_limit_k(model.max_tokens),
    )
}

fn run_models_command(json: bool) {
    let config_file = load_config();
    match config_file {
        Some(cfg) => {
            let models = cfg.models.list_models();
            if models.is_empty() {
                eprintln!("No models configured. Add models to ~/.aemeath/config.json");
                std::process::exit(1);
            }
            if json {
                let output: Vec<serde_json::Value> = models
                    .iter()
                    .map(|(provider, m)| {
                        serde_json::json!({
                            "provider": provider,
                            "id": m.id,
                            "name": m.name,
                            "context_window": m.context_window,
                            "max_tokens": m.max_tokens,
                        })
                    })
                    .collect();
                println!("{}", serde_json::to_string_pretty(&output).unwrap());
            } else {
                // 表格输出 — 自适应列宽
                let header = ("PROVIDER", "ID", "NAME", "CTX", "MAX");
                let rows: Vec<(String, String, String, String, String)> = models
                    .iter()
                    .map(|(provider, m)| model_row_display(provider, m))
                    .collect();

                let w0 = rows
                    .iter()
                    .map(|r| r.0.len())
                    .chain(std::iter::once(header.0.len()))
                    .max()
                    .unwrap_or(0);
                let w1 = rows
                    .iter()
                    .map(|r| r.1.len())
                    .chain(std::iter::once(header.1.len()))
                    .max()
                    .unwrap_or(0);
                let w2 = rows
                    .iter()
                    .map(|r| r.2.len())
                    .chain(std::iter::once(header.2.len()))
                    .max()
                    .unwrap_or(0);

                println!(
                    "{:<w$}  {:<w2$}  {:<w3$}  {:<w4$}  {}",
                    header.0,
                    header.1,
                    header.2,
                    header.3,
                    header.4,
                    w = w0,
                    w2 = w1,
                    w3 = w2,
                    w4 = header.3.len()
                );
                for (provider, id, name, ctx, max) in &rows {
                    println!(
                        "{:<w$}  {:<w2$}  {:<w3$}  {:<w4$}  {}",
                        provider,
                        id,
                        name,
                        ctx,
                        max,
                        w = w0,
                        w2 = w1,
                        w3 = w2,
                        w4 = header.3.len()
                    );
                }
            }
        }
        None => {
            eprintln!("No config file found. Create ~/.aemeath/config.json to configure models.");
            std::process::exit(1);
        }
    }
}

/// 处理 `aemeath sessions` 子命令
async fn run_sessions_command(delete: Option<String>, json: bool, limit: usize) {
    // 初始化 session ID（日志需要）
    set_session_id("sessions".to_string());

    if let Some(id) = delete {
        match aemeath_core::session::delete_session(&id).await {
            Ok(()) => println!("Session {} deleted.", id),
            Err(e) => {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
        return;
    }

    let sessions = aemeath_core::session::list_sessions().await;
    if sessions.is_empty() {
        println!("No saved sessions.");
        return;
    }

    let display: Vec<_> = sessions.into_iter().take(limit).collect();

    if json {
        let output: Vec<serde_json::Value> = display
            .iter()
            .map(|s| {
                serde_json::json!({
                    "id": s.id,
                    "title": s.metadata.title,
                    "project": s.metadata.project,
                    "model": s.metadata.model,
                    "messages": s.messages.len(),
                    "created_at": s.created_at,
                    "updated_at": s.updated_at,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&output).unwrap());
    } else {
        let header = ("ID", "SUMMARY", "PROJECT", "MSG", "UPDATED");
        let rows: Vec<(&str, String, &str, usize, &str)> = display
            .iter()
            .map(|s| {
                let summary = s.summary();
                let summary_display: String = summary.chars().take(80).collect();
                let project = s.metadata.project.as_deref().unwrap_or("-");
                let updated = s.updated_at.get(..16).unwrap_or(&s.updated_at);
                (
                    s.id.as_str(),
                    summary_display,
                    project,
                    s.messages.len(),
                    updated,
                )
            })
            .collect();

        let w0 = rows
            .iter()
            .map(|r| r.0.len())
            .chain(std::iter::once(header.0.len()))
            .max()
            .unwrap_or(0);
        let w1 = rows
            .iter()
            .map(|r| r.1.len())
            .chain(std::iter::once(header.1.len()))
            .max()
            .unwrap_or(0)
            .min(60);
        let w2 = rows
            .iter()
            .map(|r| r.2.len())
            .chain(std::iter::once(header.2.len()))
            .max()
            .unwrap_or(0);

        println!(
            "{:<w$}  {:<w2$}  {:<w3$}  {:>3}  {}",
            header.0,
            header.1,
            header.2,
            header.3,
            header.4,
            w = w0,
            w2 = w1,
            w3 = w2
        );
        for (id, summary, project, msg, updated) in &rows {
            println!(
                "{:<w$}  {:<w2$}  {:<w3$}  {:>3}  {}",
                id,
                summary,
                project,
                msg,
                updated,
                w = w0,
                w2 = w1,
                w3 = w2
            );
        }
    }
}

/// 加载配置文件（供子命令复用）
fn load_config() -> Option<aemeath_core::config::Config> {
    let paths = [
        dirs::home_dir()
            .map(|h| h.join(".aemeath").join("config.json"))
            .unwrap_or_default(),
        std::path::PathBuf::from(".aemeath/config.json"),
    ];
    for path in &paths {
        if path.exists() {
            if let Ok(content) = std::fs::read_to_string(path) {
                if let Ok(c) = serde_json::from_str::<aemeath_core::config::Config>(&content) {
                    return Some(c);
                }
            }
        }
    }
    None
}

fn select_model_for_run(
    requested_model: Option<&str>,
    config_file: Option<&Config>,
) -> Result<ResolvedModel, String> {
    let cfg = config_file.ok_or_else(|| {
        "未指定模型。请使用 --model <来源>/<模型>，或在 ~/.aemeath/config.json 配置 models.default".to_string()
    })?;

    if let Some(selection) = requested_model.filter(|s| !s.trim().is_empty()) {
        cfg.models
            .resolve_model_selection(selection)
            .map_err(|e| e.to_string())
    } else {
        cfg.models
            .resolve_default_model()
            .map_err(|e| e.to_string())
    }
}

/// 主聊天逻辑（原 main 主体）
async fn run_chat(mut args: Args) {
    // 初始化所有内置命令（自动注册到全局 CommandRegistry）
    aemeath_core::command::commands::init_all();

    // 检查 AEMEATH_PERMISSION_MODE 环境变量
    if !args.allow_all {
        if let Ok(mode) = std::env::var("AEMEATH_PERMISSION_MODE") {
            if mode == "allow_all" {
                args.allow_all = true;
            }
        }
    }

    // 加载 config.json 以获取 provider 默认值 (apiKey, baseUrl, model)
    // 优先级: CLI args > env vars > 项目 config.json > 全局 config.json > built-in defaults

    // 初始化 guidance 目录（首次运行时生成默认 guidance 文件）
    aemeath_core::guidance::init_guidance_dir();

    let cwd = args
        .cwd
        .clone()
        .or_else(|| env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."));

    let config_file = aemeath_core::config::ConfigManager::new(Some(&cwd))
        .load()
        .await
        .ok();

    // 初始化日志系统（在 config 加载之后，使用配置中的日志级别）
    init_logging(
        config_file
            .as_ref()
            .map(|c| &c.logging)
            .unwrap_or(&aemeath_core::config::LoggingConfig::default()),
    );

    // 应用 config.json 中的 permissions.mode（CLI --allow-all 和 env var 优先）
    if !args.allow_all {
        if let Some(ref cfg) = config_file {
            if matches!(
                cfg.permissions.mode,
                aemeath_core::config::PermissionModeConfig::AllowAll
            ) {
                args.allow_all = true;
            }
        }
    }

    let requested_model = args.model.as_deref();
    let resolved_model = select_model_for_run(requested_model, config_file.as_ref())
        .unwrap_or_else(|e| {
            eprintln!("Error: {e}");
            std::process::exit(1);
        });
    let api_type = resolved_model.api;

    // 获取 API key: CLI args > env vars > resolved config
    let api_key = args.api_key.take().unwrap_or_else(|| {
        std::env::var("AEMEATH_API_KEY")
            .or_else(|_| std::env::var("ANTHROPIC_API_KEY"))
            .or_else(|_| std::env::var("OPENAI_API_KEY"))
            .or_else(|_| std::env::var("LLM_API_KEY"))
            .unwrap_or_else(|_| {
                if !resolved_model.source_config.api_key.is_empty() {
                    return resolved_model.source_config.api_key.clone();
                }
                eprintln!("Error: API key not set. Use --api-key, set LLM_API_KEY, or configure in ~/.aemeath/config.json");
                std::process::exit(1);
            })
    });

    let base_url = args.base_url.clone().or_else(|| {
        if resolved_model.source_config.base_url.is_empty() {
            None
        } else {
            Some(resolved_model.source_config.base_url.clone())
        }
    });
    let model = resolved_model.model.id.clone();
    let max_tokens = args.max_tokens.unwrap_or_else(|| {
        if resolved_model.model.max_tokens > 0 {
            resolved_model.model.max_tokens
        } else if config_file
            .as_ref()
            .map(|c| c.model.max_tokens > 0)
            .unwrap_or(false)
        {
            config_file.as_ref().unwrap().model.max_tokens
        } else {
            32000
        }
    });
    let thinking_max_tokens = resolved_model.model.thinking_max_tokens;
    let reasoning = resolved_model.model.reasoning.unwrap_or(!args.no_think);

    // reasoning_effort: CLI args > config.json model entry > env var > None
    let reasoning_effort = args
        .reasoning_effort
        .clone()
        .or_else(|| resolved_model.model.reasoning_effort.clone())
        .or_else(|| std::env::var("AEMEATH_REASONING_EFFORT").ok())
        .filter(|e| !e.is_empty());
    if let Some(ref effort) = reasoning_effort {
        if let Err(e) = aemeath_core::config::models::validate_reasoning_effort(effort) {
            log::error!("{}", e);
            std::process::exit(1);
        }
    }

    log::info!(
        "[main] source={} api={} model={} reasoning={} effort={:?} args.no_think={}",
        resolved_model.source_key,
        api_type.as_str(),
        model,
        reasoning,
        reasoning_effort,
        args.no_think
    );

    let openai_config = if matches!(api_type, ApiDriverKind::Anthropic) {
        None
    } else {
        Some(OpenAIProviderConfig::from_api_driver(
            api_type,
            &resolved_model.source_key,
        ))
    };
    let reasoning_config = reasoning_effort
        .as_ref()
        .map(|effort| ReasoningConfig::Object(serde_json::json!({ "effort": effort })))
        .or_else(|| {
            if thinking_max_tokens > 0 {
                Some(ReasoningConfig::ThinkingBudget(thinking_max_tokens))
            } else {
                resolved_model.model.reasoning.map(ReasoningConfig::Bool)
            }
        });

    let client = LlmClient::from_config(
        api_type,
        api_key,
        base_url,
        model.clone(),
        max_tokens,
        thinking_max_tokens,
        reasoning,
        reasoning_config,
        openai_config,
    );
    if let Some(effort) = reasoning_effort {
        client.set_reasoning_effort(Some(effort));
    }

    let client = std::sync::Arc::new(client);

    let task_store = std::sync::Arc::new(aemeath_core::task::TaskStore::new());

    // 加载 skills
    let skill_dirs = config_file
        .as_ref()
        .map(|c| c.skills.dirs.clone())
        .unwrap_or_default();
    let skills_map = aemeath_core::skill::load_all_skills(&cwd, &skill_dirs);
    if !skills_map.is_empty() {
        log::info!("[Skills] loaded {} skills", skills_map.len());
    }
    let skills = std::sync::Arc::new(tokio::sync::Mutex::new(skills_map.clone()));
    let mut registry = ToolRegistry::new();
    aemeath_tools::register_all_tools(&mut registry, task_store.clone(), skills.clone());

    let _mcp_clients = load_mcp_tools(&mut registry, &cwd).await;

    // Create hook runner before agent_runner so it can be shared
    let cwd_str = cwd.display().to_string();
    let hook_runner = if let Some(ref cfg) = config_file {
        aemeath_core::hook::HookRunner::from_config(cfg, cwd_str.clone())
    } else {
        aemeath_core::hook::HookRunner::empty(cwd_str.clone())
    };

    let agent_runner = {
        // Build LlmClientPool if there are multiple providers configured
        let models_config_arc = std::sync::Arc::new(
            config_file
                .as_ref()
                .map(|c| c.models.clone())
                .unwrap_or_default(),
        );
        let has_multi_providers = models_config_arc.providers.len() > 1
            || !config_file
                .as_ref()
                .map(|c| c.agents.roles.is_empty())
                .unwrap_or(true);

        let pool = if has_multi_providers {
            Some(std::sync::Arc::new(aemeath_llm::LlmClientPool::new(
                client.clone(),
                models_config_arc.clone(),
            )))
        } else {
            None
        };
        let agents_config = std::sync::Arc::new(
            config_file
                .as_ref()
                .map(|c| c.agents.clone())
                .unwrap_or_default(),
        );

        std::sync::Arc::new(agent_runner::CliAgentRunner {
            client: client.clone(),
            pool,
            agents_config,
            hook_runner: hook_runner.clone(),
            reasoning,
            models_config: models_config_arc.clone(),
        })
    };

    let prompt_parts = build_system_prompt_parts(&cwd, &hook_runner).await;

    // Skills 列表加入 static part（仅在启动时变化）
    let static_prompt = {
        let skills_guard = skills.lock().await;

        // 解析 model 特定的 guidance
        let guidance_config = config_file
            .as_ref()
            .map(|c| c.models.guidance.clone())
            .unwrap_or_default();
        let model_guidance = aemeath_core::guidance::resolve_guidance_async(
            &model,
            &guidance_config,
            reasoning,
            Some(&hook_runner),
        )
        .await;

        // 组装: static_part + universal discipline + skills + model guidance (末尾锚定语言)
        let mut prompt = prompt_parts.static_part;
        prompt.push_str(aemeath_core::guidance::UNIVERSAL_EXECUTION_DISCIPLINE);
        if !skills_guard.is_empty() {
            let skill_list: Vec<String> = skills_guard
                .values()
                .map(|s| {
                    let alias_str = if s.aliases.is_empty() {
                        String::new()
                    } else {
                        format!(" (aliases: /{})", s.aliases.join(", /"))
                    };
                    format!("- `{}{}`: {}", s.name, alias_str, s.description)
                })
                .collect();
            prompt.push_str(&format!(
                "\n\n# Available Skills\nThe following skills can be invoked with the Skill tool:\n{}",
                skill_list.join("\n")
            ));
        }

        // Inject agent roles into system prompt so the main LLM knows what's available
        if let Some(ref cfg) = config_file {
            if !cfg.agents.roles.is_empty() {
                let role_lines: Vec<String> = cfg
                    .agents
                    .roles
                    .iter()
                    .map(|(name, role)| {
                        let desc = if role.description.is_empty() {
                            String::new()
                        } else {
                            format!(": {}", role.description)
                        };
                        let model_info = if role.model.is_empty() {
                            String::new()
                        } else {
                            format!(" (model: {})", role.model)
                        };
                        format!("- `{}`{}{}", name, desc, model_info)
                    })
                    .collect();
                prompt.push_str(&format!(
                    "\n\n# Available Agent Roles\nThe following agent roles are available for the Agent tool's `role` parameter. Choose the most appropriate role for each task:\n{}\nWhen no role fits, omit the `role` parameter to use the default model.",
                    role_lines.join("\n")
                ));
            }
        }

        // model guidance 放在末尾，离推理最近，最大化对 reasoning 语言的影响
        if !model_guidance.is_empty() {
            prompt.push_str("\n\n");
            prompt.push_str(&model_guidance);
        }
        prompt
    };

    // 构建 SystemBlock 数组用于 prompt caching
    use aemeath_llm::types::SystemBlock;
    let system_blocks: Vec<SystemBlock> = vec![
        SystemBlock::cached(static_prompt),
        SystemBlock::dynamic(prompt_parts.dynamic_part),
    ];

    // CLAUDE.md 上下文将作为 user message 前置注入
    let user_context = prompt_parts.claude_md;

    // 用于 compact 估算，拼接为纯文本
    let system_prompt_text = system_blocks
        .iter()
        .map(|b| b.text.as_str())
        .collect::<Vec<_>>()
        .join("\n\n");

    // 确定 session ID
    let session_id = args
        .resume
        .clone()
        .unwrap_or_else(|| aemeath_core::session::new_session_id());
    set_session_id(session_id.clone());
    log::info!("session started");

    // 解析并发限制: CLI args > config file > defaults
    let max_tool_concurrency = args
        .max_tool_concurrency
        .filter(|&v| v > 0)
        .or_else(|| {
            config_file
                .as_ref()
                .map(|c| c.tools.max_concurrency)
                .filter(|&v| v > 0)
        })
        .unwrap_or(10);
    let max_agent_concurrency = args
        .max_agent_concurrency
        .filter(|&v| v > 0)
        .or_else(|| {
            config_file
                .as_ref()
                .map(|c| c.agents.max_concurrency)
                .filter(|&v| v > 0)
        })
        .unwrap_or(4);
    let agent_semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(max_agent_concurrency));

    log::info!(
        "concurrency limits: max_tool={}, max_agent={}",
        max_tool_concurrency,
        max_agent_concurrency
    );

    // 以 TUI 模式或旧版 REPL 模式运行
    if args.no_tui {
        let memory_config = config_file
            .as_ref()
            .map(|c| c.memory.clone())
            .unwrap_or_default();
        repl::run_repl(
            client,
            registry,
            system_blocks.clone(),
            system_prompt_text.clone(),
            user_context.clone(),
            cwd,
            args.verbose,
            !args.no_markdown,
            args.context_size,
            args.resume,
            Some(agent_runner),
            args.allow_all,
            task_store.clone(),
            max_tool_concurrency,
            agent_semaphore.clone(),
            skills_map.clone(),
            hook_runner.clone(),
            memory_config,
        )
        .await;
    } else {
        // 构建显示名: provider/name (来自 config) 或仅 model id
        // provider 名称以原始 config 形式显示（不转小写），
        // 因此 `Zhipu/GLM-5.1 ⚡` 保持 `Zhipu/GLM-5.1 ⚡`
        let model_display = {
            let display_name = if resolved_model.model.name.is_empty() {
                resolved_model.model.id.as_str()
            } else {
                resolved_model.model.name.as_str()
            };
            format!("{}/{}", resolved_model.source_key, display_name)
        };
        let mut app = tui::App::new(session_id.clone(), cwd, model_display);
        app.memory_config = config_file
            .as_ref()
            .map(|c| c.memory.clone())
            .unwrap_or_default();
        app.set_skills(skills_map);
        app.hook_runner = hook_runner.clone();
        if let Err(e) = app
            .run(
                client,
                registry,
                system_blocks,
                system_prompt_text,
                user_context,
                args.context_size,
                args.verbose,
                !args.no_markdown,
                Some(agent_runner),
                args.allow_all,
                args.resume,
                task_store,
                max_tool_concurrency,
                max_agent_concurrency,
                agent_semaphore,
            )
            .await
        {
            log::error!("TUI error: {e}");
            std::process::exit(1);
        }
        println!("aemeath --resume {}", session_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aemeath_core::config::{Config, ModelEntryConfig, ModelsConfig, ProviderModelsConfig};
    use std::collections::HashMap;

    fn test_config_for_model_selection() -> Config {
        let mut providers = HashMap::new();
        providers.insert(
            "Zhipu".to_string(),
            ProviderModelsConfig {
                api: "zhipu".to_string(),
                api_key: "zhipu-key".to_string(),
                base_url: "https://zhipu.example.com".to_string(),
                models: vec![ModelEntryConfig {
                    id: "glm-5.1".to_string(),
                    max_tokens: 128000,
                    ..Default::default()
                }],
            },
        );
        providers.insert(
            "LiteLLM".to_string(),
            ProviderModelsConfig {
                api: "litellm".to_string(),
                api_key: "litellm-key".to_string(),
                base_url: "https://litellm.example.com".to_string(),
                models: vec![ModelEntryConfig {
                    id: "anthropic/claude-opus-4-7".to_string(),
                    max_tokens: 16000,
                    ..Default::default()
                }],
            },
        );
        Config {
            models: ModelsConfig {
                default: "Zhipu/glm-5.1".to_string(),
                providers,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn test_model_row_display_includes_max_tokens_as_k() {
        let model = ModelEntryConfig {
            id: "deepseek-v4-pro".to_string(),
            name: "DeepSeek V4 Pro".to_string(),
            context_window: 200_000,
            max_tokens: 8192,
            ..Default::default()
        };

        let row = model_row_display("DeepSeek", &model);

        assert_eq!(row.0, "DeepSeek");
        assert_eq!(row.1, "deepseek-v4-pro");
        assert_eq!(row.2, "DeepSeek V4 Pro");
        assert_eq!(row.3, "200k");
        assert_eq!(row.4, "8k");
    }

    #[test]
    fn test_model_row_display_zero_max_tokens_as_dash() {
        let model = ModelEntryConfig {
            id: "local".to_string(),
            context_window: 0,
            max_tokens: 0,
            ..Default::default()
        };

        let row = model_row_display("Ollama", &model);

        assert_eq!(row.2, "-");
        assert_eq!(row.3, "-");
        assert_eq!(row.4, "-");
    }

    #[test]
    fn test_select_model_prefers_cli_model() {
        let cfg = test_config_for_model_selection();
        let selected =
            select_model_for_run(Some("LiteLLM/anthropic/claude-opus-4-7"), Some(&cfg)).unwrap();
        assert_eq!(selected.source_key, "LiteLLM");
        assert_eq!(selected.model.id, "anthropic/claude-opus-4-7");
    }

    #[test]
    fn test_select_model_uses_config_default() {
        let cfg = test_config_for_model_selection();
        let selected = select_model_for_run(None, Some(&cfg)).unwrap();
        assert_eq!(selected.source_key, "Zhipu");
        assert_eq!(selected.model.id, "glm-5.1");
    }

    #[test]
    fn test_select_model_without_config_errors() {
        let err = select_model_for_run(None, None).unwrap_err();
        assert!(err.contains("未指定模型"));
    }
}
