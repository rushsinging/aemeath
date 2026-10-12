//! `aemeath scoring` 子命令 — 本机 System One scoring daemon。
//!
//! - `aemeath scoring serve` — 常驻加载 GGUF，监听 UDS（#1935）

use composition::systemone::ScoringServeExit;

pub(crate) async fn run_scoring_serve_command() {
    match composition::systemone::run_scoring_serve().await {
        ScoringServeExit::Stopped { sock } => {
            println!("scoring serve 已停止（{}）", sock.display());
        }
        ScoringServeExit::Failure { message, exit_code } => {
            eprintln!("{message}");
            std::process::exit(exit_code);
        }
    }
}
