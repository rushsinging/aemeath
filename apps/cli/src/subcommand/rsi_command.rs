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
    pub max_runs: Option<usize>,
    pub window_days: Option<u64>,
    pub json: bool,
}

/// `rsi run --json` 的稳定 JSON 面：summary 由 serde 转义，NEVER 手工拼接。
fn render_rsi_json_summary(summary: &str) -> String {
    serde_json::json!({ "summary": summary }).to_string()
}

pub(crate) async fn run_rsi_run_command(options: RsiRunOptions) {
    let overrides = composition::harness::RsiRunOverrides {
        max_runs: options.max_runs,
        window_days: options.window_days,
    };
    let result = if options.dry_run {
        composition::harness::run_rsi_run_dry(&overrides).await
    } else {
        composition::harness::run_rsi_run_once_with(&overrides).await
    };
    match result {
        composition::harness::RsiRunExit::Done { summary } if options.json => {
            println!("{}", render_rsi_json_summary(&summary));
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

/// `rsi history`（#2074）。
pub(crate) fn run_rsi_history_command(component: Option<&str>) {
    match composition::harness::run_rsi_history(component) {
        composition::harness::RsiRunExit::Done { summary } => println!("{summary}"),
        composition::harness::RsiRunExit::Failure { message, exit_code } => {
            eprintln!("{message}");
            std::process::exit(exit_code);
        }
    }
}

/// `rsi rollback`（#2074）。
pub(crate) fn run_rsi_rollback_command(component: &str, cycle: Option<&str>) {
    match composition::harness::run_rsi_rollback(component, cycle) {
        composition::harness::RsiRunExit::Done { summary } => println!("{summary}"),
        composition::harness::RsiRunExit::Failure { message, exit_code } => {
            eprintln!("{message}");
            std::process::exit(exit_code);
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn rsi_json_summary_escapes_control_characters_and_quotes() {
        let rendered = super::render_rsi_json_summary("line-1\nline-2 \"quoted\" \\ slash\tend");
        let parsed: serde_json::Value = serde_json::from_str(&rendered).expect("JSON 必须合法");
        assert_eq!(
            parsed["summary"].as_str().expect("summary"),
            "line-1\nline-2 \"quoted\" \\ slash\tend"
        );
    }
}
