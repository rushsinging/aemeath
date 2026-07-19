use clap::{Args as ClapArgs, Parser, Subcommand};
use std::path::PathBuf;

/// A Rust-based AI coding agent
#[derive(Parser)]
#[command(name = "aemeath", version = composition::COMPILED_VERSION)]
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
    #[arg(long)]
    pub api_key: Option<String>,

    /// API base URL (overrides provider-specific default)
    #[arg(long)]
    pub base_url: Option<String>,

    /// Model selection in <source>/<model> format
    #[arg(long)]
    pub model: Option<String>,

    /// Working directory
    #[arg(long)]
    pub cwd: Option<PathBuf>,

    /// Max output tokens
    #[arg(long)]
    pub max_tokens: Option<u32>,

    /// Send application logs to stderr
    #[arg(short = 'v', long)]
    pub verbose: bool,

    /// Run without the TUI; use pipe input once or interactive text REPL
    #[arg(short = 'q', long)]
    pub quiet: bool,

    /// Disable markdown rendering
    #[arg(long)]
    pub no_markdown: bool,

    /// Context window size in tokens (0 = auto-resolve from provider model config)
    #[arg(long, default_value = "0")]
    pub context_size: usize,

    /// Resume a saved session by ID
    #[arg(long)]
    pub resume: Option<String>,

    /// Skip all permission prompts (auto-approve all tool calls)
    #[arg(long = "yolo", alias = "allow-all")]
    pub allow_all: bool,

    /// Maximum number of concurrent tool executions (default: 10)
    #[arg(long)]
    pub max_tool_concurrency: Option<usize>,

    /// Maximum number of concurrent sub-agent executions (default: 4)
    #[arg(long)]
    pub max_agent_concurrency: Option<usize>,

    /// Disable reasoning/thinking mode (default: enabled)
    #[arg(long)]
    pub no_think: bool,

    /// Maximum reasoning level for compatible models (off/low/medium/high/xhigh/max)
    #[arg(long, value_name = "LEVEL")]
    pub max_reasoning: Option<String>,
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

    /// Check for and install updates
    Update {
        /// Only check for available updates, don't install
        #[arg(long)]
        check: bool,
    },

    /// Print version information
    Version,
}

/// The original Args struct, used by the rest of main.rs to avoid touching all call sites.
pub struct Args {
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub model: Option<String>,
    pub cwd: Option<PathBuf>,
    pub max_tokens: Option<u32>,
    pub verbose: bool,
    pub quiet: bool,
    pub no_markdown: bool,
    pub context_size: usize,
    pub resume: Option<String>,
    pub allow_all: bool,
    pub max_tool_concurrency: Option<usize>,
    pub max_agent_concurrency: Option<usize>,
    pub no_think: bool,
    pub max_reasoning: Option<String>,
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
            quiet: r.quiet,
            no_markdown: r.no_markdown,
            context_size: r.context_size,
            resume: r.resume,
            allow_all: r.allow_all,
            max_tool_concurrency: r.max_tool_concurrency,
            max_agent_concurrency: r.max_agent_concurrency,
            no_think: r.no_think,
            max_reasoning: r.max_reasoning,
        }
    }
}

impl From<Args> for sdk::ChatBootstrapArgs {
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
            max_reasoning: args.max_reasoning,
            logging_output: if args.quiet && args.verbose {
                // 仅 no-tui（--quiet）+ --verbose 时走 stderr；TUI 模式 stderr 会糊屏（#1215）。
                sdk::LoggingOutputMode::Stderr
            } else {
                sdk::LoggingOutputMode::File
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yolo_and_allow_all_alias_project_same_runtime_bootstrap_acl() {
        let yolo = Cli::try_parse_from(["aemeath", "--yolo"]).unwrap();
        let alias = Cli::try_parse_from(["aemeath", "--allow-all"]).unwrap();

        let yolo_bootstrap = sdk::ChatBootstrapArgs::from(Args::from(yolo.run_args));
        let alias_bootstrap = sdk::ChatBootstrapArgs::from(Args::from(alias.run_args));

        assert!(yolo_bootstrap.allow_all);
        assert!(alias_bootstrap.allow_all);
    }

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

    #[test]
    fn test_cli_accepts_quiet_short_flag() {
        let cli = Cli::try_parse_from(["aemeath", "-q"]).unwrap();

        assert!(cli.run_args.quiet);
    }

    #[test]
    fn test_cli_accepts_quiet_long_flag() {
        let cli = Cli::try_parse_from(["aemeath", "--quiet"]).unwrap();

        assert!(cli.run_args.quiet);
    }

    #[test]
    fn test_cli_accepts_verbose_short_flag() {
        let cli = Cli::try_parse_from(["aemeath", "-v"]).unwrap();

        assert!(cli.run_args.verbose);
    }

    #[test]
    fn default_cli_maps_to_file_logging_output() {
        let cli = Cli::try_parse_from(["aemeath"]).unwrap();
        let bootstrap = sdk::ChatBootstrapArgs::from(Args::from(cli.run_args));

        assert_eq!(bootstrap.logging_output, sdk::LoggingOutputMode::File);
    }

    #[test]
    fn verbose_cli_maps_to_stderr_logging_output() {
        // 仅在 no-tui（--quiet）下 --verbose 走 stderr，保留实时日志语义。
        let cli = Cli::try_parse_from(["aemeath", "--quiet", "--verbose"]).unwrap();
        let bootstrap = sdk::ChatBootstrapArgs::from(Args::from(cli.run_args));

        assert_eq!(bootstrap.logging_output, sdk::LoggingOutputMode::Stderr);
    }

    #[test]
    fn tui_verbose_stays_file_to_avoid_stderr_polluting_alternate_screen() {
        // TUI 模式（非 --quiet）下 --verbose 绝不走 stderr——stderr 会越过
        // alternate screen 的双缓冲直接糊屏（#1215）。
        let cli = Cli::try_parse_from(["aemeath", "--verbose"]).unwrap();
        let bootstrap = sdk::ChatBootstrapArgs::from(Args::from(cli.run_args));

        assert_eq!(bootstrap.logging_output, sdk::LoggingOutputMode::File);
    }

    #[test]
    fn quiet_cli_maps_to_file_logging_output() {
        let cli = Cli::try_parse_from(["aemeath", "--quiet"]).unwrap();
        let bootstrap = sdk::ChatBootstrapArgs::from(Args::from(cli.run_args));

        assert_eq!(bootstrap.logging_output, sdk::LoggingOutputMode::File);
    }

    #[test]
    fn verbose_logging_output_takes_precedence_over_quiet() {
        let cli = Cli::try_parse_from(["aemeath", "--quiet", "--verbose"]).unwrap();
        let bootstrap = sdk::ChatBootstrapArgs::from(Args::from(cli.run_args));

        assert_eq!(bootstrap.logging_output, sdk::LoggingOutputMode::Stderr);
    }

    #[test]
    fn test_args_from_run_args_carries_quiet_flag() {
        let cli = Cli::try_parse_from(["aemeath", "--quiet"]).unwrap();
        let args = Args::from(cli.run_args);

        assert!(args.quiet);
    }
}
