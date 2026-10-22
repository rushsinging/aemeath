//! composition 装配：RSI 维护 daemon 的组合根。
//!
//! 任务集（TaskKind 装配）在此唯一装配：本阶段注册空任务集
//! （改进循环实现由后续 issue 接入），daemon 框架面先稳定。

use harness::adapters::daemon_serve::{run_rsi_daemon, RsiDaemonEndpoints};
use harness::domain::task::TaskRegistry;
use std::path::Path;

/// `aemeath rsi serve` 的退出结果。
#[derive(Debug)]
pub enum RsiServeExit {
    Stopped { sock: std::path::PathBuf },
    Failure { message: String, exit_code: i32 },
}

/// 运行 RSI 维护 daemon：装配端点与任务集，阻塞至 Stop/ctrl-c。
pub async fn run_rsi_serve(on_ready: impl FnOnce(&Path)) -> RsiServeExit {
    let agents_dir = share::config::paths::global_agents_dir();
    let endpoints = RsiDaemonEndpoints::under_agents_dir(&agents_dir);
    let sock = endpoints.sock.clone();
    let tasks = TaskRegistry::new();
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let ctrl_c = tokio::signal::ctrl_c();

    on_ready(&sock);
    let daemon = run_rsi_daemon(
        endpoints,
        tasks,
        crate::constants::RSI_INTERVAL_DEFAULT,
        shutdown_rx,
    );
    tokio::select! {
        result = daemon => match result {
            Ok(()) => RsiServeExit::Stopped { sock },
            Err(error) => RsiServeExit::Failure {
                message: format!("rsi serve 运行失败：{error}"),
                exit_code: 2,
            },
        },
        _ = ctrl_c => {
            let _ = shutdown_tx.send(());
            RsiServeExit::Stopped { sock }
        },
    }
}
