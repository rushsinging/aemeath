mod application;
mod cli;
mod model_selection;
mod render;
mod repl;
mod run_orchestration;
mod sessions_command;
mod tui;

use clap::Parser;
use cli::{Cli, Commands};

#[tokio::main]
async fn main() {
    ::runtime::api::bootstrap::init_panic_hook();

    let cli = Cli::parse();

    match cli.command {
        Some(Commands::Models { json }) => {
            model_selection::run_models_command(json).await;
        }
        Some(Commands::Sessions {
            delete,
            json,
            limit,
        }) => {
            sessions_command::run_sessions_command(delete, json, limit).await;
        }
        Some(Commands::Run { run_args }) => {
            run_orchestration::run_chat(run_args.into()).await;
        }
        None => {
            // 无子命令 — 默认调用 run，使用顶层参数
            run_orchestration::run_chat(cli.run_args.into()).await;
        }
    }
}
