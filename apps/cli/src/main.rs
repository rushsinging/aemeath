mod args;
mod model_selection;
mod run_orchestration;
mod runtime_adapter;
mod sessions_command;
mod tui;

use args::{Args, Cli, Commands};
use clap::Parser;

#[tokio::main]
async fn main() {
    ::runtime::api::bootstrap::init_panic_hook();

    let cli = Cli::parse();

    match cli.command {
        Some(Commands::Models { json }) => {
            let client = runtime_adapter::agent_client_from_args(Args::from(cli.run_args).into())
                .await
                .unwrap_or_else(|e| {
                    eprintln!("Error: {e}");
                    std::process::exit(1);
                });
            model_selection::run_models_command(client, json).await;
        }
        Some(Commands::Sessions {
            delete,
            json,
            limit,
        }) => {
            let client = runtime_adapter::agent_client_from_args(Args::from(cli.run_args).into())
                .await
                .unwrap_or_else(|e| {
                    eprintln!("Error: {e}");
                    std::process::exit(1);
                });
            sessions_command::run_sessions_command(client, delete, json, limit).await;
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
