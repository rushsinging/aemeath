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

    /// Configure a built-in LLM provider interactively
    Connect,

    /// Manage System One scoring model and local daemon
    Systemone {
        #[command(subcommand)]
        command: SystemoneCommands,
    },

    /// Manage the harness RSI maintenance daemon
    Rsi {
        #[command(subcommand)]
        command: RsiCommands,
    },

    /// Remove orphaned on-disk data (sessions / tool results / workflows) that
    /// no longer has any reference. Irreversible.
    Cleanup {
        /// Only list what would be removed, delete nothing
        #[arg(long)]
        dry_run: bool,

        /// Skip the interactive confirmation prompt
        #[arg(long, short = 'y')]
        yes: bool,
    },

    /// Check for and install updates
    Update {
        /// Only check for available updates, don't install
        #[arg(long)]
        check: bool,
    },

    /// Print version information
    Version,

    /// Offline evaluation tools for the compaction chain (hidden)
    #[command(hide = true)]
    Eval {
        #[command(subcommand)]
        command: EvalCommands,
    },
}

/// `aemeath eval` 子命令集（隐藏，离线评测用，不参与日常对话链路）。
#[derive(Subcommand)]
pub enum EvalCommands {
    /// Compact chain evaluation
    #[command(hide = true)]
    Compact {
        #[command(subcommand)]
        command: CompactEvalCommands,
    },
}

/// `aemeath eval compact quiz` 子命令集（两步式）。
#[derive(Subcommand)]
pub enum QuizCommands {
    /// 出题：答题模型读 early 历史，产出题库文件（后续多档复用）
    Generate {
        /// export 产生的 case 目录
        #[arg(long)]
        cases: PathBuf,

        /// 出题模型 selection
        #[arg(long)]
        model: String,

        /// 出题推理档位（缺省 low，跟随全局默认）
        #[arg(long, default_value = "low")]
        reasoning: String,

        /// 最多为多少个 case 出题
        #[arg(long, default_value_t = 1)]
        limit: usize,

        /// 题库输出路径（JSON）
        #[arg(long)]
        output: PathBuf,
    },

    /// 答题+判卷：用固定题库，多档位可复用保证可比
    Answer {
        /// export 产生的 case 目录
        #[arg(long)]
        cases: PathBuf,

        /// generate 产出的题库文件
        #[arg(long)]
        questions: PathBuf,

        /// 压缩输出目录（<case_id>.summary.txt；可含 run_summary.json 取 tail 条数）
        #[arg(long)]
        summaries: PathBuf,

        /// 答题模型 selection
        #[arg(long)]
        model: String,

        /// 独立判卷模型 selection（评测时显式指定，不设默认）
        #[arg(long)]
        judge: String,

        /// 答题推理档位（缺省 low，跟随全局默认）
        #[arg(long, default_value = "low")]
        reasoning: String,

        /// 判卷推理档位（缺省跟随 --reasoning；判卷质量优先时可单独抬高）
        #[arg(long)]
        judge_reasoning: Option<String>,

        /// 最多评多少个 case
        #[arg(long, default_value_t = 1)]
        limit: usize,

        /// 报告输出路径（缺省打印到 stdout）
        #[arg(long)]
        output: Option<PathBuf>,
    },
}

/// `aemeath eval compact` 子命令集。
#[derive(Subcommand)]
pub enum CompactEvalCommands {
    /// 从 context.log 输出压缩画像（频率/耗时/分块/降级/模型分桶）
    Telemetry {
        /// 日志路径（默认 ~/.agents/logs/context.log）
        #[arg(long)]
        log: Option<PathBuf>,

        /// 只统计该 ISO 日期前缀之后的事件，如 2026-10-01
        #[arg(long)]
        since: Option<String>,

        /// 输出 JSON 路径（默认 eval/compact/results/telemetry_baseline.json）
        #[arg(long)]
        output: Option<PathBuf>,
    },

    /// 从真实 session dataset 导出压缩评测 case
    Export {
        /// session 根目录（默认 ~/.agents/session）
        #[arg(long)]
        root: Option<PathBuf>,

        /// case 输出目录（必填）
        #[arg(long)]
        output_dir: PathBuf,

        /// 最多导出多少个 case
        #[arg(long, default_value_t = 8)]
        limit: usize,

        /// 只导出 committed steps 不少于该数量的会话
        #[arg(long, default_value_t = 40)]
        min_steps: usize,

        /// 每个 case 的 must_keep 上限（按出现频次取前 N）
        #[arg(long, default_value_t = 1000)]
        max_must_keep: usize,

        /// must_keep 最低出现次数（默认 3；只出现一次的路径多为噪声，不计入）
        #[arg(long, default_value_t = 3)]
        min_must_keep_frequency: usize,

        /// 跳过估算 token 超过该值的会话（超出压缩窗口的巨型历史不适合评测）
        #[arg(long, default_value_t = 3_000_000)]
        max_tokens: u64,

        /// 只导出发生过压缩的会话（真实生产压缩点，quiz/生产反馈用）
        #[arg(long, default_value_t = false)]
        only_compacted: bool,

        /// 只导出被压缩历史（early）估算 token 不低于该值的会话
        #[arg(long, default_value_t = 100_000)]
        min_compacted_tokens: u64,

        /// 只导出指定会话（dataset 目录名 stem，如 01a182e6-0323-...）
        #[arg(long)]
        session: Option<String>,
    },

    /// 生产实际反馈指标（重问率/重复读取/错误率/中断/不满信号/成本）
    Feedback {
        /// session 根目录（默认 ~/.agents/session）
        #[arg(long)]
        root: Option<PathBuf>,

        /// 最多检查多少个 session
        #[arg(long, default_value_t = 200)]
        limit: usize,

        /// 输出 JSON 路径（缺省打印到 stdout）
        #[arg(long)]
        output: Option<PathBuf>,
    },

    /// 对导出的 case 跑一次真实压缩（走生产装配与管线）
    Run {
        /// export 产生的 case 目录
        #[arg(long)]
        cases: PathBuf,

        /// summary 输出目录（写 <case_id>.summary.txt 与 run_summary.json）
        #[arg(long)]
        output_dir: PathBuf,

        /// 最多跑多少个 case
        #[arg(long, default_value_t = 3)]
        limit: usize,

        /// 显式 compact 模型 selection（如 "Bailian Token Plan/deepseek-v4.1-flash-1017"）；
        /// 缺省跟随 context.compact_model
        #[arg(long)]
        model: Option<String>,

        /// 压缩调用的推理档位（off/minimal/low/medium/high/xhigh/max）；
        /// 缺省跟随 context.compact_reasoning（未配置=low）
        #[arg(long)]
        reasoning: Option<String>,
    },

    /// 续接问答（P3，两步式）：generate 出题落盘 → answer 多档复用同一题库
    Quiz {
        #[command(subcommand)]
        command: QuizCommands,
    },

    /// 真实压缩摘要的 must_keep 召回与压缩率（用已落盘摘要，不重跑压缩）
    Recall {
        /// session 根目录（默认 ~/.agents/session）
        #[arg(long)]
        root: Option<PathBuf>,

        /// 最多检查多少个 session
        #[arg(long, default_value_t = 200)]
        limit: usize,

        /// 每个 session 的 must_keep 上限（按出现频次取前 N）
        #[arg(long, default_value_t = 1000)]
        max_must_keep: usize,

        /// 输出 JSON 路径（缺省打印到 stdout）
        #[arg(long)]
        output: Option<PathBuf>,
    },

    /// 按 case 计算确定性指标（需要压缩输出）
    Score {
        /// export 产生的 case 目录
        #[arg(long)]
        cases: PathBuf,

        /// 压缩输出目录，文件名约定 <case_id>.summary.txt
        #[arg(long)]
        summaries: Option<PathBuf>,

        /// 输出 JSON 路径
        #[arg(long)]
        output: Option<PathBuf>,
    },
}

/// `aemeath systemone` 子命令集。
#[derive(Subcommand)]
pub enum SystemoneCommands {
    /// Download and install the System One scoring model (~/.agents/models/systemone)
    Download,
    /// 启动本机 scoring daemon（加载一份 GGUF，经 UDS 服务多会话）
    Serve,
}

/// `aemeath rsi` 子命令集。
#[derive(Subcommand)]
pub enum RsiCommands {
    /// 一次性执行单轮改进周期后退出（手动触发与验收入口）
    Run {
        /// 预演模式：完整周期但零持久副作用（组件/lesson/checkpoint 全不写），
        /// 预览落 ~/.agents/harness/dry-run-preview.md（覆盖写）
        #[arg(long)]
        dry_run: bool,
        /// 覆盖 evidence_max_runs_per_cycle（仅本次；单位是 run）
        #[arg(long, alias = "max-sessions")]
        max_runs: Option<usize>,
        /// 覆盖 evidence_initial_window_days（仅本次；仅无 checkpoint 时生效）
        #[arg(long)]
        window_days: Option<u64>,
        /// 结果以 JSON 输出（机器可读）
        #[arg(long)]
        json: bool,
    },
    /// 启动 RSI 维护 daemon（常驻；定时改进 harness 组件）
    Serve,
    /// 列出组件快照历史（回滚依据）
    History {
        /// 限定组件（<kind>/<name>）；省略则列全部
        #[arg(long)]
        component: Option<String>,
    },
    /// 回滚组件到快照（默认最近一次采纳前）
    Rollback {
        /// 组件标识（<kind>/<name>）
        component: String,
        /// 指定 cycle id（省略则用最近快照）
        #[arg(long)]
        cycle: Option<String>,
    },
}

/// The original Args struct, used by the rest of main.rs to avoid touching all call sites.
#[derive(Clone)]
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
    /// 启动即打开 Connect 向导（`aemeath connect`）。
    pub startup_connect: bool,
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
            startup_connect: false,
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
            native_stderr: if args.quiet {
                sdk::NativeStderrMode::Preserve
            } else {
                sdk::NativeStderrMode::RouteToLogs
            },
        }
    }
}

#[cfg(test)]
#[path = "args_tests.rs"]
mod tests;
