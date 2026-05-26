use clap::{Args as ClapArgs, Parser, Subcommand};
use std::path::PathBuf;

/// A Rust-based AI coding agent
#[derive(Parser)]
#[command(name = "aemeath")]
pub struct Cli {
    #[command(flatten)]
    pub run_args: RunArgs,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

/// Run options (shared between top-level `aemeath` and `aemeath run` subcommand)
#[derive(ClapArgs)]
pub struct RunArgs {
    /// API key (overrides provider-specific env var)
    #[arg(long, env = "AEMEATH_API_KEY")]
    pub api_key: Option<String>,

    /// API base URL (overrides provider-specific default)
    #[arg(long, env = "AEMEATH_BASE_URL")]
    pub base_url: Option<String>,

    /// Model selection in <source>/<model> format (overrides AEMEATH_MODEL)
    #[arg(long, env = "AEMEATH_MODEL")]
    pub model: Option<String>,

    /// Working directory
    #[arg(long)]
    pub cwd: Option<PathBuf>,

    /// Max output tokens
    #[arg(long)]
    pub max_tokens: Option<u32>,

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

    /// Maximum number of concurrent tool executions (default: 10)
    #[arg(long, env = "AEMEATH_MAX_TOOL_CONCURRENCY")]
    pub max_tool_concurrency: Option<usize>,

    /// Maximum number of concurrent sub-agent executions (default: 4)
    #[arg(long, env = "AEMEATH_MAX_AGENT_CONCURRENCY")]
    pub max_agent_concurrency: Option<usize>,

    /// Disable reasoning/thinking mode (default: enabled)
    #[arg(long)]
    pub no_think: bool,

    /// Reasoning effort level for compatible models (none/low/medium/high/xhigh)
    #[arg(long)]
    pub reasoning_effort: Option<String>,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Start chat session (default when no subcommand is given)
    #[command(hide = true)]
    Run {
        #[command(flatten)]
        run_args: RunArgs,
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

/// The original Args struct, used by the rest of main.rs to avoid touching all call sites.
pub struct Args {
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
    pub max_tool_concurrency: Option<usize>,
    pub max_agent_concurrency: Option<usize>,
    pub no_think: bool,
    pub reasoning_effort: Option<String>,
}
impl From<RunArgs> for Args {
    fn from(r: RunArgs) -> Self {
        Self {
            api_key: r.api_key,
            base_url: r.base_url,
            model: r.model,
            cwd: r.cwd,
            max_tokens: r.max_tokens,
            verbose: r.verbose,
            no_markdown: r.no_markdown,
            context_size: r.context_size,
            resume: r.resume,
            allow_all: r.allow_all,
            max_tool_concurrency: r.max_tool_concurrency,
            max_agent_concurrency: r.max_agent_concurrency,
            no_think: r.no_think,
            reasoning_effort: r.reasoning_effort,
        }
    }
}

impl From<Args> for ::runtime::api::bootstrap::ChatBootstrapArgs {
    fn from(args: Args) -> Self {
        Self {
            api_key: args.api_key,
            base_url: args.base_url,
            model: args.model,
            cwd: args.cwd,
            max_tokens: args.max_tokens,
            verbose: args.verbose,
            no_markdown: args.no_markdown,
            context_size: args.context_size,
            resume: args.resume,
            allow_all: args.allow_all,
            max_tool_concurrency: args.max_tool_concurrency,
            max_agent_concurrency: args.max_agent_concurrency,
            no_think: args.no_think,
            reasoning_effort: args.reasoning_effort,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_rejects_provider_argument() {
        assert!(Cli::try_parse_from(["aemeath", "--provider", "Zhipu"]).is_err());
    }

    #[test]
    fn test_cli_accepts_model_selection() {
        let cli = Cli::try_parse_from(["aemeath", "--model", "Zhipu/glm-5.1"]).unwrap();

        assert_eq!(cli.run_args.model.as_deref(), Some("Zhipu/glm-5.1"));
    }

    #[test]
    fn test_args_from_run_args_has_no_provider_field_requirement() {
        let cli = Cli::try_parse_from(["aemeath", "--model", "LiteLLM/anthropic/claude-opus-4-7"])
            .unwrap();
        let args = Args::from(cli.run_args);

        assert_eq!(
            args.model.as_deref(),
            Some("LiteLLM/anthropic/claude-opus-4-7")
        );
    }
}
