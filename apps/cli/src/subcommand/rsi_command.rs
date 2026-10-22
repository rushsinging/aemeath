//! `aemeath rsi` 子命令入口。

pub(crate) async fn run_rsi_serve_command() {
    let exit = composition::harness::run_rsi_serve(|sock| {
        println!("rsi serve 监听 {}", sock.display());
    })
    .await;
    match exit {
        composition::harness::RsiServeExit::Stopped { sock } => {
            println!("rsi serve 已停止（{}）", sock.display());
        }
        composition::harness::RsiServeExit::Failure { message, exit_code } => {
            eprintln!("{message}");
            std::process::exit(exit_code);
        }
    }
}

pub(crate) async fn run_rsi_run_command() {
    match composition::harness::run_rsi_run_once().await {
        composition::harness::RsiRunExit::Done { summary } => {
            println!("rsi run 完成：{summary}");
        }
        composition::harness::RsiRunExit::Failure { message, exit_code } => {
            eprintln!("{message}");
            std::process::exit(exit_code);
        }
    }
}
