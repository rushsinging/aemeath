mod agent_runner;
mod cli;
mod image;
mod mcp_loader;
mod prompt;
mod render;
mod repl;
mod tui;

use aemeath_core::provider::Provider;
use aemeath_core::tool::ToolRegistry;
use aemeath_llm::client::LlmClient;
use clap::Parser;
use std::env;
use std::path::PathBuf;
use std::sync::OnceLock;

/// 全局 session ID，供日志格式化器使用
static SESSION_ID: OnceLock<String> = OnceLock::new();

/// 设置全局 session ID（只能调用一次）
fn set_session_id(id: String) {
    let _ = SESSION_ID.set(id);
}

use cli::{Args, Cli, Commands};
use mcp_loader::load_mcp_tools;
use prompt::build_system_prompt_parts;

#[tokio::main]
async fn main() {
    // 初始化结构化日志 — 路由到 ~/.aemeath/aemeath.log，避免库的 log::warn! / log::error! 破坏 TUI 渲染
    // 设置 AEMEATH_LOG_STDERR=1 可在使用 --no-tui / CLI 模式调试时恢复旧的 stderr 行为
    {
        let mut builder = env_logger::Builder::from_env(
            env_logger::Env::default().default_filter_or("warn,aemeath_llm=debug,aemeath_cli=debug"),
        );
        let use_stderr = std::env::var("AEMEATH_LOG_STDERR")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
        if !use_stderr {
            let log_path = dirs::home_dir()
                .unwrap_or_default()
                .join(".aemeath")
                .join("aemeath.log");
            if let Some(parent) = log_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Ok(file) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&log_path)
            {
                builder.target(env_logger::Target::Pipe(Box::new(file)));
            }
        }
        builder.format(|buf, record| {
            use std::io::Write;
            let session = SESSION_ID.get().map(|s| s.as_str()).unwrap_or("????????");
            writeln!(
                buf,
                "[{} {} {}] {}",
                buf.timestamp(),
                session,
                record.level(),
                record.args()
            )
        });
        builder.init();
    }

    // 设置 panic hook：将 panic 信息写入日志文件 + stderr
    {
        let log_path = dirs::home_dir()
            .unwrap_or_default()
            .join(".aemeath")
            .join("panic.log");
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

            let msg = format!("[PANIC] {} at {}", payload, location);

            // 写日志文件
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&log_path)
            {
                use std::io::Write;
                let _ = writeln!(f, "[{:?}] {}", std::time::SystemTime::now(), msg);
                // 写 backtrace
                let _ = writeln!(f, "Backtrace:\n{:?}", std::backtrace::Backtrace::capture());
            }

            // 同时写 stderr（非 TUI 模式可见）
            eprintln!("{}", msg);
        }));
    }

    let cli = Cli::parse();

    match cli.command {
        Some(Commands::Models { json }) => {
            run_models_command(json);
            return;
        }
        Some(Commands::Sessions { delete, json, limit }) => {
            run_sessions_command(delete, json, limit).await;
            return;
        }
        Some(Commands::Run {
            provider,
            api_key,
            base_url,
            model,
            cwd,
            max_tokens,
            verbose,
            no_markdown,
            context_size,
            resume,
            allow_all,
            tui,
            no_tui,
            max_tool_concurrency,
            max_agent_concurrency,
            no_think,
        }) => {
            let args = Args::from_run(
                provider, api_key, base_url, model, cwd, max_tokens, verbose,
                no_markdown, context_size, resume, allow_all, tui, no_tui,
                max_tool_concurrency, max_agent_concurrency, no_think,
            );
            run_chat(args).await;
        }
        None => {
            // 无子命令 — 使用默认值启动（兼容旧行为）
            let args = Args::from_run(
                "anthropic".into(), None, None, None, None, 200000, false,
                false, 128000, None, false, true, false,
                None, None, false,
            );
            run_chat(args).await;
        }
    }
}

/// 处理 `aemeath models` 子命令
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
                let output: Vec<serde_json::Value> = models.iter().map(|(provider, m)| {
                    serde_json::json!({
                        "provider": provider,
                        "id": m.id,
                        "name": m.name,
                        "context_window": m.context_window,
                        "max_tokens": m.max_tokens,
                    })
                }).collect();
                println!("{}", serde_json::to_string_pretty(&output).unwrap());
            } else {
                // 表格输出 — 自适应列宽
                let header = ("PROVIDER", "ID", "NAME", "CTX");
                let rows: Vec<(&str, &str, &str, String)> = models.iter().map(|(provider, m)| {
                    let name = if m.name.is_empty() { "-" } else { m.name.as_str() };
                    let ctx = if m.context_window > 0 {
                        format!("{}k", m.context_window / 1000)
                    } else {
                        "-".to_string()
                    };
                    (provider.as_str(), m.id.as_str(), name, ctx)
                }).collect();

                let w0 = rows.iter().map(|r| r.0.len()).chain(std::iter::once(header.0.len())).max().unwrap_or(0);
                let w1 = rows.iter().map(|r| r.1.len()).chain(std::iter::once(header.1.len())).max().unwrap_or(0);
                let w2 = rows.iter().map(|r| r.2.len()).chain(std::iter::once(header.2.len())).max().unwrap_or(0);

                println!("{:<w$}  {:<w2$}  {:<w3$}  {}", header.0, header.1, header.2, header.3, w = w0, w2 = w1, w3 = w2);
                for (provider, id, name, ctx) in &rows {
                    println!("{:<w$}  {:<w2$}  {:<w3$}  {}", provider, id, name, ctx, w = w0, w2 = w1, w3 = w2);
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
        let output: Vec<serde_json::Value> = display.iter().map(|s| {
            serde_json::json!({
                "id": s.id,
                "title": s.metadata.title,
                "project": s.metadata.project,
                "model": s.metadata.model,
                "messages": s.messages.len(),
                "created_at": s.created_at,
                "updated_at": s.updated_at,
            })
        }).collect();
        println!("{}", serde_json::to_string_pretty(&output).unwrap());
    } else {
        let header = ("ID", "SUMMARY", "PROJECT", "MSG", "UPDATED");
        let rows: Vec<(&str, String, &str, usize, &str)> = display.iter().map(|s| {
            let summary = s.summary();
            let summary_display: String = summary.chars().take(80).collect();
            let project = s.metadata.project.as_deref().unwrap_or("-");
            let updated = s.updated_at.get(..16).unwrap_or(&s.updated_at);
            (s.id.as_str(), summary_display, project, s.messages.len(), updated)
        }).collect();

        let w0 = rows.iter().map(|r| r.0.len()).chain(std::iter::once(header.0.len())).max().unwrap_or(0);
        let w1 = rows.iter().map(|r| r.1.len()).chain(std::iter::once(header.1.len())).max().unwrap_or(0).min(60);
        let w2 = rows.iter().map(|r| r.2.len()).chain(std::iter::once(header.2.len())).max().unwrap_or(0);

        println!("{:<w$}  {:<w2$}  {:<w3$}  {:>3}  {}", header.0, header.1, header.2, header.3, header.4, w = w0, w2 = w1, w3 = w2);
        for (id, summary, project, msg, updated) in &rows {
            println!("{:<w$}  {:<w2$}  {:<w3$}  {:>3}  {}", id, summary, project, msg, updated, w = w0, w2 = w1, w3 = w2);
        }
    }
}

/// 加载配置文件（供子命令复用）
fn load_config() -> Option<aemeath_core::config::Config> {
    let paths = [
        dirs::home_dir().map(|h| h.join(".aemeath").join("config.json")).unwrap_or_default(),
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
    // 优先级: CLI args > env vars > config.json > built-in defaults

    // 初始化 guidance 目录（首次运行时生成默认 guidance 文件）
    aemeath_core::guidance::init_guidance_dir();

    let config_file = {
        let paths = [
            dirs::home_dir().map(|h| h.join(".aemeath").join("config.json")).unwrap_or_default(),
            std::path::PathBuf::from(".aemeath/config.json"),
        ];
        let mut cfg: Option<aemeath_core::config::Config> = None;
        for path in &paths {
            if path.exists() {
                if let Ok(content) = std::fs::read_to_string(path) {
                    if let Ok(c) = serde_json::from_str::<aemeath_core::config::Config>(&content) {
                        cfg = Some(c);
                        break;
                    }
                }
            }
        }
        cfg
    };

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

    // 应用 config.json 默认值（当 CLI/env 未指定时）
    // Provider + model: 仅在 CLI 使用默认值且未设置 env var 时覆盖
    // 保存已解析的 ModelEntryConfig 以便获取 id 和 reasoning 标志
    let mut config_default_model: Option<(String, aemeath_core::config::ModelEntryConfig)> = None;
    if args.provider == "anthropic" && std::env::var("AEMEATH_PROVIDER").is_err() {
        if let Some(ref cfg) = config_file {
            if !cfg.models.default.is_empty() {
                // 解析 "provider/model_query" 格式 — find_model 先按 id 再按 name 匹配（含模糊回退）
                if let Some((provider_name, model_query)) = cfg.models.default.split_once('/') {
                    args.provider = provider_name.to_string();
                    if args.model.is_none() && std::env::var("AEMEATH_MODEL").is_err() {
                        if let Some((_pn, _pc, model_entry)) = cfg.models.find_model(&cfg.models.default) {
                            config_default_model = Some((model_entry.id.clone(), model_entry));
                        } else {
                            // 未匹配 — 拒绝将 display name 作为 model id 发送到 API（会导致 "Model Not Exist"）
                            // 列出可用模型帮助用户修正配置
                            let available: Vec<String> = cfg.models.providers
                                .get(provider_name)
                                .map(|p| p.models.iter()
                                    .map(|m| format!("{} (id: {})", m.name, m.id))
                                    .collect())
                                .unwrap_or_default();
                            log::error!(
                                "models.default '{}' does not match any configured model under provider '{}'.\n  query: {}\n  available models:\n    {}",
                                cfg.models.default,
                                provider_name,
                                model_query,
                                if available.is_empty() {
                                    "(none — no models configured for this provider)".to_string()
                                } else {
                                    available.join("\n    ")
                                },
                            );
                            std::process::exit(1);
                        }
                    }
                } else {
                    // 仅有 provider 名，无 model
                    args.provider = cfg.models.default.clone();
                }
            } else {
                // 回退: 使用第一个有 models 的 provider
                for (name, pcfg) in &cfg.models.providers {
                    if !pcfg.models.is_empty() {
                        args.provider = name.clone();
                        break;
                    }
                }
            }
        }
    }

    // 解析 provider
    let provider = Provider::from_str(&args.provider).unwrap_or_else(|| {
        log::error!("Unknown provider '{}'. Use one of: anthropic, openai, openrouter, deepseek, moonshot, zhipu, dashscope, minimax, ollama, openai-compatible", args.provider);
        std::process::exit(1);
    });

    // 获取 API key: CLI args > env vars > config.json
    let api_key = args.api_key.unwrap_or_else(|| {
        let env_key = provider.api_key_env();
        std::env::var(env_key).unwrap_or_else(|_| {
            // 回退: 尝试 ANTHROPIC_API_KEY（兼容旧版）
            if provider == Provider::Anthropic {
                if let Ok(key) = std::env::var("ANTHROPIC_API_KEY") {
                    return key;
                }
            }
            // 回退: 尝试 config.json 中匹配的 provider
            if let Some(ref cfg) = config_file {
                // 按精确 provider 名匹配
                if let Some(pcfg) = cfg.models.provider_ci(&args.provider) {
                    if !pcfg.api_key.is_empty() {
                        return pcfg.api_key.clone();
                    }
                }
                // 尝试任何 provider（如果只有一个或第一个匹配）
                for (_, pcfg) in &cfg.models.providers {
                    if !pcfg.api_key.is_empty() {
                        return pcfg.api_key.clone();
                    }
                }
            }
            log::error!("API key not set. Use --api-key, set {}, or configure in ~/.aemeath/config.json", env_key);
            std::process::exit(1);
        })
    });

    let cwd = args
        .cwd
        .or_else(|| env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."));

    // 获取 model: CLI args > env var > config.json default > config.json provider > provider default
    let model = args.model.unwrap_or_else(|| {
        // 1. 来自 models.default（通过 find_model 解析）
        if let Some((ref model_id, _)) = config_default_model {
            return model_id.clone();
        }
        // 2. 来自 config.json provider 的第一个 model
        if let Some(ref cfg) = config_file {
            if let Some(pcfg) = cfg.models.provider_ci(&args.provider) {
                if let Some(first_model) = pcfg.models.first() {
                    return first_model.id.clone();
                }
            }
        }
        // 3. 硬编码默认值（仅当用户没有显式指定 provider 且无 config 时）
        if args.provider != "anthropic" {
            log::error!(
                "No model configured for provider '{}'. Add a model configuration to ~/.aemeath/config.json, or specify --model.",
                args.provider
            );
            std::process::exit(1);
        }
        provider.default_model().to_string()
    });

    // 获取 base_url: CLI args > env var > config.json > provider default
    if args.base_url.is_none() && std::env::var("AEMEATH_BASE_URL").is_err() {
        if let Some(ref cfg) = config_file {
            if let Some(pcfg) = cfg.models.provider_ci(&args.provider) {
                if !pcfg.base_url.is_empty() {
                    args.base_url = Some(pcfg.base_url.clone());
                }
            }
        }
    }

    // 将 max_tokens 限制在 provider 上限内
    let max_tokens = {
        let limit = provider.max_output_tokens();
        if limit > 0 && args.max_tokens > limit {
            log::info!("max_tokens {} exceeds provider limit, clamped to {}", args.max_tokens, limit);
            limit
        } else {
            args.max_tokens
        }
    };

    let client = LlmClient::with_provider(
        provider,
        api_key,
        args.base_url,
        Some(model.clone()),
        max_tokens,
        !args.no_think, // reasoning defaults to on, --no-think disables it
    );

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
                      .unwrap_or_default()
              );
              let has_multi_providers = models_config_arc.providers.len() > 1
                  || !config_file.as_ref().map(|c| c.agents.roles.is_empty()).unwrap_or(true);

              let pool = if has_multi_providers {
                  Some(std::sync::Arc::new(aemeath_llm::LlmClientPool::new(
                      client.clone(),
                      models_config_arc,
                  )))
              } else {
                  None
              };

              let agents_config = std::sync::Arc::new(
                  config_file
                      .as_ref()
                      .map(|c| c.agents.clone())
                      .unwrap_or_default()
              );

              std::sync::Arc::new(agent_runner::CliAgentRunner {
                  client: client.clone(),
                  pool,
                  agents_config,
                  hook_runner: hook_runner.clone(),
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
        let reasoning = !args.no_think;
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
            let skill_list: Vec<String> = skills_guard.values()
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
                let role_lines: Vec<String> = cfg.agents.roles.iter()
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
    let system_prompt_text = system_blocks.iter().map(|b| b.text.as_str()).collect::<Vec<_>>().join("\n\n");

    // 确定 session ID
    let session_id = args.resume.clone().unwrap_or_else(|| aemeath_core::session::new_session_id());
    set_session_id(session_id.clone());
    log::info!("session started");

    // 解析并发限制: CLI args > config file > defaults
    let max_tool_concurrency = args.max_tool_concurrency
        .filter(|&v| v > 0)
        .or_else(|| config_file.as_ref().map(|c| c.tools.max_concurrency).filter(|&v| v > 0))
        .unwrap_or(10);
    let max_agent_concurrency = args.max_agent_concurrency
        .filter(|&v| v > 0)
        .or_else(|| config_file.as_ref().map(|c| c.agents.max_concurrency).filter(|&v| v > 0))
        .unwrap_or(4);
    let agent_semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(max_agent_concurrency));

    // 将并发限制记录到 debug.log 以便诊断
    {
        use std::io::Write;
        let debug_path = dirs::home_dir().unwrap_or_default().join(".aemeath").join("debug.log");
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&debug_path) {
            let _ = writeln!(f, "[{}] concurrency limits: max_tool={}, max_agent={}",
                std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0),
                max_tool_concurrency, max_agent_concurrency);
        }
    }
    log::info!("concurrency limits: max_tool={}, max_agent={}", max_tool_concurrency, max_agent_concurrency);

          // 以 TUI 模式或旧版 REPL 模式运行
    if args.no_tui {
        repl::run_repl(client, registry, system_blocks.clone(), system_prompt_text.clone(), user_context.clone(), cwd, args.verbose, !args.no_markdown, args.context_size, args.resume, Some(agent_runner), args.allow_all, task_store.clone(), max_tool_concurrency, agent_semaphore.clone(), skills_map.clone(), hook_runner.clone()).await;
    } else {
        // 构建显示名: provider/name (来自 config) 或仅 model id
        // provider 名称以原始 config 形式显示（不转小写），
        // 因此 `Zhipu/GLM-5.1 ⚡` 保持 `Zhipu/GLM-5.1 ⚡`
        let model_display = {
            let provider_name = args.provider.as_str();
            let display_name = config_default_model
                .as_ref()
                .and_then(|(_, entry)| {
                    if entry.name.is_empty() { None } else { Some(entry.name.as_str()) }
                })
                .or_else(|| {
                    config_file.as_ref().and_then(|cfg| {
                        cfg.models.provider_ci(provider_name).and_then(|pcfg| {
                            pcfg.models.iter().find(|m| m.id == model)
                                .and_then(|m| if m.name.is_empty() { None } else { Some(m.name.as_str()) })
                        })
                    })
                })
                .unwrap_or(&model);
            format!("{}/{}", provider_name, display_name)
        };
        let mut app = tui::App::new(session_id.clone(), cwd, model_display);
        app.set_skills(skills_map);
        app.hook_runner = hook_runner.clone();
        if let Err(e) = app.run(client, registry, system_blocks, system_prompt_text, user_context, args.context_size, args.verbose, !args.no_markdown, Some(agent_runner), args.allow_all, args.resume, task_store, max_tool_concurrency, max_agent_concurrency, agent_semaphore).await {
            log::error!("TUI error: {e}");
            std::process::exit(1);
        }
        println!("aemeath --resume {}", session_id);
    }
}
