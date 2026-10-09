//! `aemeath systemone` 子命令 — 内嵌 System One 评分模型管理。
//!
//! - `aemeath systemone download` — 下载并安装评分模型（幂等；已安装则跳过）
//!
//! 本模块只解析后的参数转发给 composition 装配链并渲染结果；
//! 模型下载 / 校验 / 安装全部经 `composition::systemone` 完成。

use composition::systemone::SystemoneDownloadExit;

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
