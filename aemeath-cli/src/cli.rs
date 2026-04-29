use clap::{Parser, Subcommand};
use std::path::PathBuf;

/// A Rust-based AI coding agent
#[derive(Parser)]
#[command(name = "aemeath")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Start chat session (default when no subcommand is given)
    #[command(hide = true)]
    Run {
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
        #[arg(long)]
        max_tokens: Option<u32>,

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

        /// Maximum number of concurrent tool executions (default: 10)
        #[arg(long, env = "AEMEATH_MAX_TOOL_CONCURRENCY")]
        max_tool_concurrency: Option<usize>,

        /// Maximum number of concurrent sub-agent executions (default: 4)
        #[arg(long, env = "AEMEATH_MAX_AGENT_CONCURRENCY")]
        max_agent_concurrency: Option<usize>,

        /// Disable reasoning/thinking mode (default: enabled)
        #[arg(long)]
        no_think: bool,
    },

    /// List available models from config
    Models {
        /// Output in JSON format
        #[arg(long)]
        json: bool,
    },

    /// Manage saved sessions
    Sessions {
        /// Delete a session by ID
        #[arg(long)]
        delete: Option<String>,

        /// Output in JSON format
        #[arg(long)]
        json: bool,

        /// Limit number of sessions shown (default: 20)
        #[arg(long, default_value = "20")]
        limit: usize,
    },
}

/// The original Args struct, reconstructed from the Run subcommand fields.
/// Used by the rest of main.rs to avoid touching all call sites.
pub struct Args {
    pub provider: String,
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub model: Option<String>,
    pub cwd: Option<PathBuf>,
    pub max_tokens: Option<u32>,
    pub verbose: bool,
    pub no_markdown: bool,
    pub context_size: usize,
    pub resume: Option<String>,
    pub allow_all: bool,
    #[allow(dead_code)]
    pub tui: bool,
    pub no_tui: bool,
    pub max_tool_concurrency: Option<usize>,
    pub max_agent_concurrency: Option<usize>,
    pub no_think: bool,
}

impl Args {
    /// Construct Args from the Run subcommand fields.
    pub fn from_run(
        provider: String,
        api_key: Option<String>,
        base_url: Option<String>,
        model: Option<String>,
        cwd: Option<PathBuf>,
        max_tokens: Option<u32>,
        verbose: bool,
        no_markdown: bool,
        context_size: usize,
        resume: Option<String>,
        allow_all: bool,
        tui: bool,
        no_tui: bool,
        max_tool_concurrency: Option<usize>,
        max_agent_concurrency: Option<usize>,
        no_think: bool,
    ) -> Self {
        Self {
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
        }
    }
}
