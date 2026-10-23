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

/// rsi run 参数面（#2050）。
pub(crate) struct RsiRunOptions {
    pub dry_run: bool,
    pub max_sessions: Option<usize>,
    pub window_days: Option<u64>,
    pub json: bool,
}

pub(crate) async fn run_rsi_run_command(options: RsiRunOptions) {
    let overrides = composition::harness::RsiRunOverrides {
        max_sessions: options.max_sessions,
        window_days: options.window_days,
    };
    let result = if options.dry_run {
        composition::harness::run_rsi_run_dry(&overrides).await
    } else {
        composition::harness::run_rsi_run_once_with(&overrides).await
    };
    match result {
        composition::harness::RsiRunExit::Done { summary } if options.json => {
            println!("{{\"summary\":\"{summary}\"}}");
        }
        composition::harness::RsiRunExit::Done { summary } => {
            println!("rsi run 完成：{summary}");
        }
        composition::harness::RsiRunExit::Failure { message, exit_code } => {
            eprintln!("{message}");
            std::process::exit(exit_code);
        }
    }
}
