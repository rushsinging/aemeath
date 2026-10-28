//! `aemeath eval compact` 子命令树（离线评测，隐藏命令）。
//!
//! 只读生产数据，不修改会话与日志。执行面（真实压缩）留待后续接入。

mod cases;
mod constants;
mod dataset;
mod score;
mod telemetry;

use crate::args::{CompactEvalCommands, EvalCommands};

/// 默认日志路径：`~/.agents/logs/context.log`。
fn default_log_path() -> std::path::PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".agents/logs/context.log")
}

/// 默认 session 根目录：`~/.agents/session`。
fn default_session_root() -> std::path::PathBuf {
    dirs::home_dir().unwrap_or_default().join(".agents/session")
}

pub(crate) fn run(command: EvalCommands) {
    match command {
        EvalCommands::Compact { command } => run_compact(command),
    }
}

fn run_compact(command: CompactEvalCommands) {
    let result = match command {
        CompactEvalCommands::Telemetry { log, since, output } => {
            telemetry::run(log.unwrap_or_else(default_log_path), since, output)
        }
        CompactEvalCommands::Export {
            root,
            output_dir,
            limit,
            min_steps,
            max_must_keep,
            max_tokens,
        } => cases::run(
            root.unwrap_or_else(default_session_root),
            output_dir,
            limit,
            min_steps,
            max_must_keep,
            max_tokens,
        ),
        CompactEvalCommands::Score {
            cases,
            summaries,
            output,
        } => score::run(cases, summaries, output),
    };
    if let Err(error) = result {
        crate::fatal_error::report_fatal(error);
    }
}
