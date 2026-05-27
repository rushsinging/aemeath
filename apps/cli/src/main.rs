mod args;
mod model_selection;
mod chat;
mod sessions_command;
mod tui;
mod panic_hook;

use args::{Args, Cli, Commands};
use clap::Parser;

#[tokio::main]
async fn main() {
    panic_hook::init_panic_hook();

    let cli = Cli::parse();

    match cli.command {
        Some(Commands::Models { json }) => {
            let client = chat::agent_client_from_args(
                sdk::ChatBootstrapArgs::from(Args::from(cli.run_args)),
            )
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
            let client = chat::agent_client_from_args(
                sdk::ChatBootstrapArgs::from(Args::from(cli.run_args)),
            )
            .await
            .unwrap_or_else(|e| {
                eprintln!("Error: {e}");
                std::process::exit(1);
            });
            sessions_command::run_sessions_command(client, delete, json, limit).await;
        }
        Some(Commands::Run { run_args }) => {
            chat::run_chat(run_args.into()).await;
        }
        None => {
            // 无子命令 — 默认调用 run，使用顶层参数
            chat::run_chat(cli.run_args.into()).await;
        }
    }
}
