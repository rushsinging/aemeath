use clap::Parser;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "aemeath", about = "A Rust-based AI coding agent")]
pub struct Args {
    /// LLM provider to use (anthropic, openai, openrouter, deepseek, moonshot, zhipu, dashscope, minimax, ollama, openai-compatible)
    #[arg(long, env = "AEMEATH_PROVIDER", default_value = "anthropic")]
    pub provider: String,

    /// API key (overrides provider-specific env var)
    #[arg(long, env = "AEMEATH_API_KEY")]
    pub api_key: Option<String>,

    /// API base URL (overrides provider-specific default)
    #[arg(long, env = "AEMEATH_BASE_URL")]
    pub base_url: Option<String>,

    /// Model to use (overrides AEMEATH_MODEL env var)
    #[arg(long, env = "AEMEATH_MODEL")]
    pub model: Option<String>,

    /// Working directory
    #[arg(long)]
    pub cwd: Option<PathBuf>,

    /// Max output tokens
    #[arg(long, default_value = "200000")]
    pub max_tokens: u32,

    /// Print raw SSE data for debugging
    #[arg(long)]
    pub verbose: bool,

    /// Disable markdown rendering
    #[arg(long)]
    pub no_markdown: bool,

    /// Context window size in tokens
    #[arg(long, env = "AEMEATH_CONTEXT_SIZE", default_value = "128000")]
    pub context_size: usize,

    /// Resume a saved session by ID
    #[arg(long)]
    pub resume: Option<String>,

    /// Skip all permission prompts (auto-approve all tool calls)
    #[arg(long)]
    pub allow_all: bool,

    /// Use TUI mode (default: true, use --no-tui for legacy REPL)
    #[arg(long, default_value = "true")]
    pub tui: bool,

    /// Disable TUI mode and use legacy REPL
    #[arg(long)]
    pub no_tui: bool,

    /// Maximum number of concurrent tool executions (default: 10)
    #[arg(long, env = "AEMEATH_MAX_TOOL_CONCURRENCY")]
    pub max_tool_concurrency: Option<usize>,

    /// Maximum number of concurrent sub-agent executions (default: 4)
    #[arg(long, env = "AEMEATH_MAX_AGENT_CONCURRENCY")]
    pub max_agent_concurrency: Option<usize>,
}
