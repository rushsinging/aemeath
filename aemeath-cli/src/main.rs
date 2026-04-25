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

use cli::Args;
use mcp_loader::load_mcp_tools;
use prompt::build_system_prompt_parts;

#[tokio::main]
async fn main() {
    // 初始化结构化日志 — 路由到 ~/.aemeath/aemeath.log，避免库的 log::warn! / log::error! 破坏 TUI 渲染
    // 设置 AEMEATH_LOG_STDERR=1 可在使用 --no-tui / CLI 模式调试时恢复旧的 stderr 行为
    {
        let mut builder = env_logger::Builder::from_env(
            env_logger::Env::default().default_filter_or("warn,aemeath_llm=debug"),
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
        builder.init();
    }

    let mut args = Args::parse();

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
        })
    };

    let prompt_parts = build_system_prompt_parts(&cwd).await;

    // Skills 列表加入 static part（仅在启动时变化）
    let static_prompt = {
        let skills_guard = skills.lock().await;

        // 解析 model 特定的 guidance
        let guidance_config = config_file
            .as_ref()
            .map(|c| c.models.guidance.clone())
            .unwrap_or_default();
        let reasoning = !args.no_think;
        let model_guidance = aemeath_core::guidance::resolve_guidance(
            &model,
            &guidance_config,
            reasoning,
        );

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
        repl::run_repl(client, registry, system_blocks.clone(), system_prompt_text.clone(), user_context.clone(), cwd, args.verbose, !args.no_markdown, args.context_size, args.resume, Some(agent_runner), args.allow_all, task_store.clone(), max_tool_concurrency, agent_semaphore.clone(), skills_map.clone()).await;
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
            if let Err(e) = app.run(client, registry, system_blocks, system_prompt_text, user_context, args.context_size, args.verbose, !args.no_markdown, Some(agent_runner), args.allow_all, args.resume, task_store, max_tool_concurrency, max_agent_concurrency, agent_semaphore).await {
                log::error!("TUI error: {e}");
                std::process::exit(1);
            }
            println!("aemeath --resume {}", session_id);
        }
    }
