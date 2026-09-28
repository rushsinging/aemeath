//! 进程级致命错误的唯一出口。
//!
//! TUI 模式下 logging 装配会把原生 stderr 路由到 `~/.agents/logs/native-stderr.log`，
//! 此时裸 `eprintln!` 的错误用户完全看不到，表现为「命令敲下去没反应」。所有以非零
//! 状态终止的启动 / 装配 / 子命令错误 **MUST** 经本模块输出：先恢复原生终端，再打印，
//! 最后排空异步诊断日志。

use std::fmt::Display;

/// 打印致命错误并以状态码 1 终止进程，统一加 `Error: ` 前缀。
pub(crate) fn report_fatal(message: impl Display) -> ! {
    exit_with_fatal(&format!("Error: {message}"))
}

/// 打印已带上下文文案的致命错误并终止进程（保留调用方原文案）。
pub(crate) fn report_fatal_message(message: impl Display) -> ! {
    exit_with_fatal(&message.to_string())
}

fn exit_with_fatal(line: &str) -> ! {
    let restore_failure = composition::app::restore_terminal_stderr().err();
    eprintln!("{line}");
    if let Some(restore_error) = restore_failure {
        eprintln!("（终端 stderr 恢复失败：{restore_error}）");
    }
    composition::app::flush_diagnostic_logs();
    std::process::exit(1);
}
