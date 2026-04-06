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
    /// LLM provider to use (anthropic, openai, openrouter, deepseek, moonshot, zhipu, dashscope, minimax, openai-compatible)
    #[arg(long, env = "AEMEATH_PROVIDER", default_value = "anthropic")]
    provider: String,

    /// API key (overrides provider-specific env var)
    #[arg(long)]
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

    parts.join("\n\n")
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
    // Truncate to 2000 chars like TS version
    if result.len() > 2000 {
        result[..2000].to_string()
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

    // Look for MCP config in .mcp.json or ~/.claude/mcp.json
    let config_paths = [
        cwd.join(".mcp.json"),
        dirs::home_dir()
            .map(|h| h.join(".claude").join("mcp.json"))
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
                eprintln!(
                    "warning: invalid MCP config {}: {e}",
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
                    eprintln!("warning: invalid MCP server config '{}': {e}", name);
                    continue;
                }
            };

            eprintln!("[MCP] connecting to {}...", name);
            match McpClient::connect(name, &mcp_config).await {
                Ok(client) => {
                    let client =
                        std::sync::Arc::new(tokio::sync::Mutex::new(client));

                    // Fetch and register tools
                    match client.lock().await.list_tools().await {
                        Ok(tools) => {
                            eprintln!("[MCP] {} registered {} tools", name, tools.len());
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
                        Err(e) => eprintln!("[MCP] {} failed to list tools: {e}", name),
                    }

                    clients.push(client);
                }
                Err(e) => eprintln!("[MCP] failed to connect to {}: {e}", name),
            }
        }
    }

    clients
}

#[tokio::main]
async fn main() {
    let args = Args::parse();

    // Parse provider
    let provider = Provider::from_str(&args.provider).unwrap_or_else(|| {
        eprintln!("error: Unknown provider '{}'. Use one of: anthropic, openai, openrouter, deepseek, moonshot, zhipu, dashscope, minimax, openai-compatible", args.provider);
        std::process::exit(1);
    });

    // Get API key from args or environment
    let api_key = args.api_key.unwrap_or_else(|| {
        // Try provider-specific env var first
        let env_key = provider.api_key_env();
        std::env::var(env_key).unwrap_or_else(|_| {
            // Legacy fallback for anthropic
            if provider == Provider::Anthropic {
                std::env::var("ANTHROPIC_API_KEY").unwrap_or_else(|_| {
                    eprintln!("error: API key not set. Use --api-key or set {}.", env_key);
                    std::process::exit(1);
                })
            } else {
                eprintln!("error: API key not set. Use --api-key or set {}.", env_key);
                std::process::exit(1);
            }
        })
    });

    let cwd = args
        .cwd
        .or_else(|| env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."));

    // Get model from args or provider default
    let model = args.model.unwrap_or_else(|| provider.default_model().to_string());

    let client = LlmClient::with_provider(
        provider,
        api_key,
        args.base_url,
        Some(model.clone()),
        args.max_tokens,
    );

    let client = std::sync::Arc::new(client);

    let task_store = std::sync::Arc::new(aemeath_core::task::TaskStore::new());

    // Load skills
    let skills_map = aemeath_core::skill::load_all_skills(&cwd);
    if !skills_map.is_empty() {
        eprintln!("[Skills] loaded {} skills", skills_map.len());
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
        if skills_guard.is_empty() {
            prompt_parts.static_part
        } else {
            let skill_list: Vec<String> = skills_guard.values()
                .map(|s| format!("- {}: {}", s.name, s.description))
                .collect();
            format!("{}\n\n# Available Skills\nThe following skills can be invoked with the Skill tool:\n{}", prompt_parts.static_part, skill_list.join("\n"))
        }
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
        repl::run_repl(client, registry, system_blocks.clone(), system_prompt_text.clone(), user_context.clone(), cwd, args.verbose, !args.no_markdown, args.context_size, args.resume, Some(agent_runner), args.allow_all).await;
    } else {
        let mut app = tui::App::new(session_id, cwd, model);
        if let Err(e) = app.run(client, registry, system_blocks, system_prompt_text, user_context, args.context_size, args.verbose, !args.no_markdown, Some(agent_runner), args.allow_all).await {
            eprintln!("TUI error: {e}");
            std::process::exit(1);
        }
    }
}
