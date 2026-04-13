mod agent_runner;
mod image;
mod render;
mod repl;
mod tui;

use aemeath_core::provider::Provider;
use aemeath_core::tool::ToolRegistry;
use aemeath_llm::client::LlmClient;
use clap::Parser;
use std::env;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "aemeath", about = "A Rust-based AI coding agent")]
struct Args {
    /// LLM provider to use (anthropic, openai, openrouter, deepseek, moonshot, zhipu, dashscope, minimax, ollama, openai-compatible)
    #[arg(long, env = "AEMEATH_PROVIDER", default_value = "anthropic")]
    provider: String,

    /// API key (overrides provider-specific env var)
    #[arg(long, env = "AEMEATH_API_KEY")]
    api_key: Option<String>,

    /// API base URL (overrides provider-specific default)
    #[arg(long, env = "AEMEATH_BASE_URL")]
    base_url: Option<String>,

    /// Model to use (overrides AEMEATH_MODEL env var)
    #[arg(long, env = "AEMEATH_MODEL")]
    model: Option<String>,

    /// Working directory
    #[arg(long)]
    cwd: Option<PathBuf>,

    /// Max output tokens
    #[arg(long, default_value = "200000")]
    max_tokens: u32,

    /// Print raw SSE data for debugging
    #[arg(long)]
    verbose: bool,

    /// Disable markdown rendering
    #[arg(long)]
    no_markdown: bool,

    /// Context window size in tokens
    #[arg(long, env = "AEMEATH_CONTEXT_SIZE", default_value = "128000")]
    context_size: usize,

    /// Resume a saved session by ID
    #[arg(long)]
    resume: Option<String>,

    /// Skip all permission prompts (auto-approve all tool calls)
    #[arg(long)]
    allow_all: bool,

    /// Use TUI mode (default: true, use --no-tui for legacy REPL)
    #[arg(long, default_value = "true")]
    tui: bool,

    /// Disable TUI mode and use legacy REPL
    #[arg(long)]
    no_tui: bool,
}

/// System prompt split into a static (cacheable) part and a dynamic (per-session) part.
pub struct SystemPromptParts {
    /// Static instructions that rarely change — eligible for prompt caching.
    pub static_part: String,
    /// Dynamic context (date, git status) that changes per session.
    pub dynamic_part: String,
    /// CLAUDE.md content, injected separately as a user-context message.
    pub claude_md: String,
}

async fn build_system_prompt_parts(cwd: &PathBuf) -> SystemPromptParts {
    let cwd_str = cwd.to_string_lossy();
    let is_git = is_git_repo(cwd).await;

    // --- Static part: instructions that don't change between sessions ---
    let static_part = format!(
        r#"You are an interactive agent that helps users with software engineering tasks. Use the instructions below and the tools available to you to assist the user.

# System
 - All text you output outside of tool use is displayed to the user.
 - You can call multiple tools in a single response. If you intend to call multiple tools and there are no dependencies between them, make all independent tool calls in parallel.
 - Do NOT use the Bash to run commands when a relevant dedicated tool is provided:
  - To read files use Read instead of cat, head, tail, or sed
  - To edit files use Edit instead of sed or awk
  - To create files use Write instead of cat with heredoc or echo redirection
  - To search for files use Glob instead of find or ls
  - To search the content of files, use Grep instead of grep or rg
 - Tool results and user messages may include <system-reminder> tags. These tags contain useful context automatically added by the system.

# Doing tasks
 - In general, do not propose changes to code you haven't read. If a user asks about or wants you to modify a file, read it first.
 - Do not create files unless they're absolutely necessary for achieving your goal.
 - Be careful not to introduce security vulnerabilities such as command injection, XSS, SQL injection.
 - Don't add features, refactor code, or make improvements beyond what was asked.

# Using Agent tool — MANDATORY two-phase approach
Sub-agents have a small context window (~128K tokens) and max 10 tool rounds. They CANNOT review an entire crate or directory.
When a task requires understanding a large codebase (review, refactor, audit, etc.):
 Phase 1 — YOU do the overview:
  - Use Glob to list files
  - Use Read(limit: 30) to skim key files
  - Use Grep to find specific patterns
  - Identify which specific files need deeper analysis
 Phase 2 — Launch FOCUSED agents:
  - Each agent reviews 1-3 SPECIFIC files (give exact paths)
  - Give each agent a SPECIFIC question to answer
  - Do NOT set max_turns unless you have a specific reason — the default (50) works well for most tasks
  - Example: Agent("Review error handling in compact.rs and token_estimation.rs — check edge cases in compaction_urgency and needs_compaction")
 NEVER launch an agent with a vague prompt like "review the core module" or "review all files in X directory".

# Todo workflow — MANDATORY
When you use TodoWrite to create todos, call TodoRun ONCE — it handles everything automatically:
- Resolves dependencies (blocked_by) and determines execution order
- Dispatches independent todos in parallel via sub-agents
- Waits for completion, unlocks downstream todos, dispatches next batch
NEVER execute todos yourself by launching Agent calls or other tools directly.

Use blocked_by to set dependencies: e.g. todo #3 depends on #1 and #2 completing first.

BAD:  TodoWrite(3 todos) → Agent("do task 1") → Agent("do task 2") → Agent("do task 3")
GOOD: TodoWrite(3 todos with dependencies) → TodoRun()

# Tone and style
 - Your responses should be short and concise.
 - Do not use emojis unless the user explicitly requests it.

# Environment
 - Working directory: {cwd_str}
 - Is a git repository: {is_git}"#
    );

    // --- Dynamic part: session-specific context ---
    let mut dynamic = String::new();

    let date = current_date();
    dynamic.push_str(&format!("# currentDate\nToday's date is {date}."));

    if is_git {
        let git_context = collect_git_context(cwd).await;
        if !git_context.is_empty() {
            dynamic.push_str("\n\n");
            dynamic.push_str(&git_context);
        }
    }

    // --- CLAUDE.md: will be injected as a separate user-context message ---
    let claude_md = load_claude_md(cwd).await;

    SystemPromptParts {
        static_part,
        dynamic_part: dynamic,
        claude_md,
    }
}

fn current_date() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let days = now / 86400;
    let mut y = 1970i64;
    let mut d = days as i64;
    loop {
        let diy = if y % 4 == 0 && (y % 100 != 0 || y % 400 == 0) { 366 } else { 365 };
        if d < diy { break; }
        d -= diy;
        y += 1;
    }
    let leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
    let md = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut m = 0usize;
    for dim in &md {
        if d < *dim as i64 { break; }
        d -= *dim as i64;
        m += 1;
    }
    format!("{:04}-{:02}-{:02}", y, m + 1, d + 1)
}

async fn is_git_repo(cwd: &PathBuf) -> bool {
    use tokio::process::Command;
    Command::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .current_dir(cwd)
        .output()
        .await
        .map(|o| o.status.success())
        .unwrap_or(false)
}

async fn load_claude_md(cwd: &PathBuf) -> String {
    let mut parts: Vec<String> = Vec::new();

    // Check project-level CLAUDE.md
    let project_path = cwd.join("CLAUDE.md");
    if project_path.exists() {
        if let Ok(content) = tokio::fs::read_to_string(&project_path).await {
            parts.push(content);
        }
    }

    // Check home directory ~/.claude/CLAUDE.md (global instructions)
    if let Some(home) = dirs::home_dir() {
        let global_path = home.join(".claude").join("CLAUDE.md");
        if global_path.exists() {
            if let Ok(content) = tokio::fs::read_to_string(&global_path).await {
                parts.push(content);
            }
        }
    }

    let mut claude_md = parts.join("\n\n");

    // Scan CLAUDE.md for prompt injection
    let warnings = aemeath_core::security::scan_content("CLAUDE.md", &claude_md);
    if !warnings.is_empty() {
        for w in &warnings {
            log::warn!("[Security] {} in {} line {}: {}", w.threat_type, w.filename, w.line_number, w.matched_text);
        }
        if let Some(prefix) = aemeath_core::security::format_warnings(&warnings) {
            claude_md = format!("{}\n\n{}", prefix, claude_md);
        }
    }

    claude_md
}

async fn collect_git_context(cwd: &PathBuf) -> String {
    use tokio::process::Command;

    let mut parts: Vec<String> = Vec::new();
    parts.push("# Git Context".to_string());

    // Branch name
    if let Ok(output) = Command::new("git")
        .args(["branch", "--show-current"])
        .current_dir(cwd)
        .output()
        .await
    {
        let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !branch.is_empty() {
            parts.push(format!("Current branch: {branch}"));
        }
    }

    // Default branch
    if let Ok(output) = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "origin/HEAD"])
        .current_dir(cwd)
        .output()
        .await
    {
        let default = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !default.is_empty() && default != "origin/HEAD" {
            let branch = default.strip_prefix("origin/").unwrap_or(&default);
            parts.push(format!("Default branch: {branch}"));
        }
    }

    // Git user
    if let Ok(output) = Command::new("git")
        .args(["config", "user.name"])
        .current_dir(cwd)
        .output()
        .await
    {
        let name = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !name.is_empty() {
            parts.push(format!("Git user: {name}"));
        }
    }

    // Status (short)
    if let Ok(output) = Command::new("git")
        .args(["--no-optional-locks", "status", "--short"])
        .current_dir(cwd)
        .output()
        .await
    {
        let status = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !status.is_empty() {
            let lines: Vec<&str> = status.lines().take(20).collect();
            parts.push(format!("Status:\n{}", lines.join("\n")));
        }
    }

    // Recent commits
    if let Ok(output) = Command::new("git")
        .args(["--no-optional-locks", "log", "--oneline", "-n", "5"])
        .current_dir(cwd)
        .output()
        .await
    {
        let log = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !log.is_empty() {
            parts.push(format!("Recent commits:\n{log}"));
        }
    }

    let result = parts.join("\n");
    // Truncate to ~2000 bytes, respecting UTF-8 char boundaries
    if result.len() > 2000 {
        let mut end = 2000;
        while end > 0 && !result.is_char_boundary(end) {
            end -= 1;
        }
        result[..end].to_string()
    } else {
        result
    }
}

async fn load_mcp_tools(
    registry: &mut ToolRegistry,
    cwd: &PathBuf,
) -> Vec<std::sync::Arc<tokio::sync::Mutex<aemeath_core::mcp::McpClient>>> {
    use aemeath_core::mcp::{McpClient, McpServerConfig};
    use aemeath_tools::mcp_tool::McpTool;

    let mut clients = Vec::new();

    // Look for MCP config in .mcp.json or ~/.aemeath/mcp.json
    let config_paths = [
        cwd.join(".mcp.json"),
        dirs::home_dir()
            .map(|h| h.join(".aemeath").join("mcp.json"))
            .unwrap_or_default(),
    ];

    for config_path in &config_paths {
        if !config_path.exists() {
            continue;
        }

        let content = match tokio::fs::read_to_string(config_path).await {
            Ok(c) => c,
            Err(_) => continue,
        };

        let config: serde_json::Value = match serde_json::from_str(&content) {
            Ok(c) => c,
            Err(e) => {
                log::warn!(
                    "invalid MCP config {}: {e}",
                    config_path.display()
                );
                continue;
            }
        };

        // Expect format: { "mcpServers": { "name": { "command": "...", "args": [...] } } }
        let servers = match config.get("mcpServers").and_then(|v| v.as_object()) {
            Some(s) => s,
            None => continue,
        };

        for (name, server_config) in servers {
            let mcp_config: McpServerConfig = match serde_json::from_value(server_config.clone()) {
                Ok(c) => c,
                Err(e) => {
                    log::warn!("invalid MCP server config '{}': {e}", name);
                    continue;
                }
            };

            log::info!("[MCP] connecting to {}...", name);
            match McpClient::connect(name, &mcp_config).await {
                Ok(client) => {
                    let client =
                        std::sync::Arc::new(tokio::sync::Mutex::new(client));

                    // Fetch and register tools
                    match client.lock().await.list_tools().await {
                        Ok(tools) => {
                            log::info!("[MCP] {} registered {} tools", name, tools.len());
                            for tool_def in tools {
                                let qualified =
                                    format!("mcp__{}_{}", name, tool_def.name);
                                registry.register(Box::new(McpTool {
                                    tool_name: tool_def.name,
                                    qualified_name: qualified,
                                    tool_description: tool_def.description,
                                    schema: tool_def.input_schema,
                                    client: client.clone(),
                                }));
                            }
                        }
                        Err(e) => log::warn!("[MCP] {} failed to list tools: {e}", name),
                    }

                    clients.push(client);
                }
                Err(e) => log::warn!("[MCP] failed to connect to {}: {e}", name),
            }
        }
    }

    clients
}

#[tokio::main]
async fn main() {
    // Initialize structured logging — route to ~/.aemeath/aemeath.log so that
    // log::warn! / log::error! from library crates (e.g. aemeath-tools) do not
    // corrupt the TUI rendering. Set AEMEATH_LOG_STDERR=1 to get the old
    // stderr behavior when debugging with `--no-tui` / CLI mode.
    {
        let mut builder = env_logger::Builder::from_env(
            env_logger::Env::default().default_filter_or("warn"),
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

    // Check AEMEATH_PERMISSION_MODE env var for allow-all
    if !args.allow_all {
        if let Ok(mode) = std::env::var("AEMEATH_PERMISSION_MODE") {
            if mode == "allow_all" {
                args.allow_all = true;
            }
        }
    }

    // Load config.json for provider defaults (apiKey, baseUrl, model)
    // Priority: CLI args > env vars > config.json > built-in defaults
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

    // Apply permissions.mode from config.json (CLI --allow-all and env var take precedence)
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

    // Apply config.json defaults where CLI/env didn't specify
    // Provider + model: only override if CLI has defaults and env vars aren't set
    // Stores the resolved ModelEntryConfig so we can get both id and reasoning flag
    let mut config_default_model: Option<(String, aemeath_core::config::ModelEntryConfig)> = None;
    if args.provider == "anthropic" && std::env::var("AEMEATH_PROVIDER").is_err() {
        if let Some(ref cfg) = config_file {
            if !cfg.models.default.is_empty() {
                // Parse "provider/model_query" format — find_model matches by id then name
                if let Some((provider_name, _model_query)) = cfg.models.default.split_once('/') {
                    args.provider = provider_name.to_string();
                    if args.model.is_none() && std::env::var("AEMEATH_MODEL").is_err() {
                        if let Some((_pn, _pc, model_entry)) = cfg.models.find_model(&cfg.models.default) {
                            config_default_model = Some((model_entry.id.clone(), model_entry));
                        } else {
                            // Fallback: use the raw query as model id
                            config_default_model = Some((_model_query.to_string(), Default::default()));
                        }
                    }
                } else {
                    // Just a provider name without model
                    args.provider = cfg.models.default.clone();
                }
            } else {
                // Fallback: use the first provider that has models
                for (name, pcfg) in &cfg.models.providers {
                    if !pcfg.models.is_empty() {
                        args.provider = name.clone();
                        break;
                    }
                }
            }
        }
    }

    // Parse provider
    let provider = Provider::from_str(&args.provider).unwrap_or_else(|| {
        log::error!("Unknown provider '{}'. Use one of: anthropic, openai, openrouter, deepseek, moonshot, zhipu, dashscope, minimax, ollama, openai-compatible", args.provider);
        std::process::exit(1);
    });

    // Get API key: CLI args > env vars > config.json
    let api_key = args.api_key.unwrap_or_else(|| {
        let env_key = provider.api_key_env();
        std::env::var(env_key).unwrap_or_else(|_| {
            // Fallback: try ANTHROPIC_API_KEY for legacy
            if provider == Provider::Anthropic {
                if let Ok(key) = std::env::var("ANTHROPIC_API_KEY") {
                    return key;
                }
            }
            // Fallback: try config.json for matching provider
            if let Some(ref cfg) = config_file {
                // Try exact provider name match
                let provider_name = args.provider.to_lowercase();
                if let Some(pcfg) = cfg.models.providers.get(&provider_name) {
                    if !pcfg.api_key.is_empty() {
                        return pcfg.api_key.clone();
                    }
                }
                // Try any provider (if there's only one or first match)
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

    // Get model: CLI args > env var > config.json default > config.json provider > provider default
    let model = args.model.unwrap_or_else(|| {
        // 1. From models.default (resolved via find_model)
        if let Some((ref model_id, _)) = config_default_model {
            return model_id.clone();
        }
        // 2. From config.json provider's first model
        let provider_name = args.provider.to_lowercase();
        if let Some(ref cfg) = config_file {
            if let Some(pcfg) = cfg.models.providers.get(&provider_name) {
                if let Some(first_model) = pcfg.models.first() {
                    return first_model.id.clone();
                }
            }
        }
        // 3. Built-in default
        provider.default_model().to_string()
    });

    // Get base_url: CLI args > env var > config.json > provider default
    if args.base_url.is_none() && std::env::var("AEMEATH_BASE_URL").is_err() {
        let provider_name = args.provider.to_lowercase();
        if let Some(ref cfg) = config_file {
            if let Some(pcfg) = cfg.models.providers.get(&provider_name) {
                if !pcfg.base_url.is_empty() {
                    args.base_url = Some(pcfg.base_url.clone());
                }
            }
        }
    }

    // Clamp max_tokens to provider limit if set
    let max_tokens = {
        let limit = provider.max_output_tokens();
        if limit > 0 && args.max_tokens > limit {
            log::info!("max_tokens {} exceeds provider limit, clamped to {}", args.max_tokens, limit);
            limit
        } else {
            args.max_tokens
        }
    };

    // Resolve reasoning flag: from config_default_model if available, otherwise lookup by model id
    let reasoning = config_default_model
        .as_ref()
        .map(|(_, entry)| entry.reasoning)
        .unwrap_or_else(|| {
            let provider_name = args.provider.to_lowercase();
            config_file.as_ref().and_then(|cfg| {
                cfg.models.providers.get(&provider_name).and_then(|pcfg| {
                    pcfg.models.iter().find(|m| m.id == model).map(|m| m.reasoning)
                })
            }).unwrap_or(false)
        });

    let client = LlmClient::with_provider(
        provider,
        api_key,
        args.base_url,
        Some(model.clone()),
        max_tokens,
        reasoning,
    );

    let client = std::sync::Arc::new(client);

    let task_store = std::sync::Arc::new(aemeath_core::task::TaskStore::new());

    // Load skills
    let skills_map = aemeath_core::skill::load_all_skills(&cwd);
    if !skills_map.is_empty() {
        log::info!("[Skills] loaded {} skills", skills_map.len());
    }
    let skills = std::sync::Arc::new(tokio::sync::Mutex::new(skills_map));

    let mut registry = ToolRegistry::new();
    aemeath_tools::register_all_tools(&mut registry, task_store.clone(), skills.clone());

    let _mcp_clients = load_mcp_tools(&mut registry, &cwd).await;

    let agent_runner = std::sync::Arc::new(agent_runner::CliAgentRunner {
        client: client.clone(),
    });

    let prompt_parts = build_system_prompt_parts(&cwd).await;

    // Skills list goes into the static part (changes only at startup)
    let static_prompt = {
        let skills_guard = skills.lock().await;

        // Resolve model-specific guidance
        let guidance_config = config_file
            .as_ref()
            .map(|c| c.models.guidance.clone())
            .unwrap_or_default();
        let provider_name = args.provider.to_lowercase();
        let model_guidance = aemeath_core::guidance::resolve_guidance(
            &provider_name,
            &model,
            &guidance_config,
        );

        // Assemble: static_part + universal discipline + model guidance + skills
        let mut prompt = prompt_parts.static_part;
        prompt.push_str("\n\n");
        prompt.push_str(aemeath_core::guidance::UNIVERSAL_EXECUTION_DISCIPLINE);
        if !model_guidance.is_empty() {
            prompt.push_str("\n\n");
            prompt.push_str(&model_guidance);
        }
        if !skills_guard.is_empty() {
            let skill_list: Vec<String> = skills_guard.values()
                .map(|s| format!("- {}: {}", s.name, s.description))
                .collect();
            prompt.push_str(&format!(
                "\n\n# Available Skills\nThe following skills can be invoked with the Skill tool:\n{}",
                skill_list.join("\n")
            ));
        }
        prompt
    };

    // Build SystemBlock array for prompt caching
    use aemeath_llm::types::SystemBlock;
    let system_blocks: Vec<SystemBlock> = vec![
        SystemBlock::cached(static_prompt),
        SystemBlock::dynamic(prompt_parts.dynamic_part),
    ];

    // CLAUDE.md context to be prepended as a user message
    let user_context = prompt_parts.claude_md;

    // For compact estimation, join as plain text
    let system_prompt_text = system_blocks.iter().map(|b| b.text.as_str()).collect::<Vec<_>>().join("\n\n");

    // Determine session ID
    let session_id = args.resume.clone().unwrap_or_else(|| aemeath_core::session::new_session_id());

    // Run in TUI mode or legacy REPL mode
    if args.no_tui {
        repl::run_repl(client, registry, system_blocks.clone(), system_prompt_text.clone(), user_context.clone(), cwd, args.verbose, !args.no_markdown, args.context_size, args.resume, Some(agent_runner), args.allow_all, task_store.clone()).await;
    } else {
        // Build display name: provider/name (from config) or just model id
        let model_display = {
            let provider_name = args.provider.to_lowercase();
            let display_name = config_default_model
                .as_ref()
                .and_then(|(_, entry)| {
                    if entry.name.is_empty() { None } else { Some(entry.name.as_str()) }
                })
                .or_else(|| {
                    config_file.as_ref().and_then(|cfg| {
                        cfg.models.providers.get(&provider_name).and_then(|pcfg| {
                            pcfg.models.iter().find(|m| m.id == model)
                                .and_then(|m| if m.name.is_empty() { None } else { Some(m.name.as_str()) })
                        })
                    })
                })
                .unwrap_or(&model);
            format!("{}/{}", provider_name, display_name)
        };
        let mut app = tui::App::new(session_id.clone(), cwd, model_display);
            if let Err(e) = app.run(client, registry, system_blocks, system_prompt_text, user_context, args.context_size, args.verbose, !args.no_markdown, Some(agent_runner), args.allow_all, args.resume, task_store).await {
                log::error!("TUI error: {e}");
                std::process::exit(1);
            }
            println!("aemeath --resume {}", session_id);
        }
    }
