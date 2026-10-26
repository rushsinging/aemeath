//! `aemeath systemone` 子命令 — System One 模型管理与本机 scoring daemon。
//!
//! - `aemeath systemone download` — 下载并安装评分模型（幂等；已安装则跳过）
//! - `aemeath systemone serve` — 常驻加载 GGUF，监听 HTTP（#2096：UDS 退役，HTTP 替代）
//!
//! 本模块只解析后的参数转发给 composition 装配链并渲染结果；
//! 模型下载 / 校验 / 安装 / daemon serve 全部经 `composition::systemone` 完成。

use composition::systemone::{ScoringServeExit, SystemoneDownloadExit};

pub(crate) async fn run_systemone_download_command(user_agent: String) {
    match composition::systemone::run_systemone_download(&user_agent).await {
        SystemoneDownloadExit::Success(report) => {
            if report.already_installed {
                println!(
                    "✓ System One 模型已安装（revision {}）：{}",
                    report.revision,
                    report.install_root.display()
                );
            } else {
                println!(
                    "✓ System One 模型下载完成（revision {}）：{}",
                    report.revision,
                    report.install_root.display()
                );
            }
        }
        SystemoneDownloadExit::Failure { message, exit_code } => {
            eprintln!("System One 模型下载失败：{message}");
            std::process::exit(exit_code);
        }
    }
}

pub(crate) async fn run_systemone_serve_command(args: crate::args::Args) {
    let http_listen = composition::app::configured_scoring_http_listen(args.into())
        .await
        .unwrap_or_else(|error| crate::fatal_error::report_fatal(error));
    let exit = composition::systemone::run_scoring_serve(&http_listen, |listen| {
        println!("systemone serve 监听 http://{listen}");
    })
    .await;
    match exit {
        ScoringServeExit::Stopped { listen } => {
            println!("systemone serve 已停止（http://{listen}）");
        }
        ScoringServeExit::Failure { message, exit_code } => {
            eprintln!("{message}");
            std::process::exit(exit_code);
        }
    }
}
